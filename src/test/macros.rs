/// Test the procedural `#[derive(Table)]` macro that `squail_macros` provides.
#[test]
fn test_table_derive_macro() {
    use squail_macros::Table;

    /// An example struct to use as rows in a database table
    #[derive(Table, Clone, Debug, Default, PartialEq, Eq)]
    struct Person {
        id: Option<i64>,
        name: String,
        age: i64,
        position: Point,
    }

    /// Custom type that does not implement ToSql/FromSql by default
    #[derive(Clone, Default, Debug, PartialEq, Eq)]
    struct Point {
        x: i16,
        y: i16
    }

    impl rusqlite::types::ToSql for Point {
        fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
            Ok(format!("{} {}", self.x, self.y).into())
        }
    }

    impl rusqlite::types::FromSql for Point {
        fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
            match value {
                rusqlite::types::ValueRef::Text(v) => {
                    let values = String::from_utf8(v.into()).unwrap();
                    let values = values.split_ascii_whitespace()
                        .map(|w| w.parse::<i16>().unwrap())
                        .collect::<Vec<_>>();

                    Ok(Self {
                        x: values[0],
                        y: values[1],
                    })
                },
                t => panic!("Wrong SQL type recieved for query of Point. Should be Text but is {:?}", t),
            }
        }
    }
    let mut larry = Person {
        id: None,
        name: String::from("larry"),
        age: 24,
        position: Point { x: 1, y: 1 },
    };

    let conn = rusqlite::Connection::open_in_memory().unwrap();

    Person::create_table(&conn).unwrap();

    larry.insert(&conn).unwrap();
    let larry_id = larry.id.unwrap();

    let larry_copy = Person::get_by_id(&conn, larry_id).unwrap();
    assert_eq!(larry_copy, Some(larry.clone()));

    let deleted_something = larry.delete(&conn).unwrap();
    // also works: `Person::delete_by_id(&conn, larry_id).unwrap();`
    assert!(deleted_something);

    let deleted_larry = Person::get_by_id(&conn, larry_id).unwrap();
    assert_eq!(deleted_larry, None);
}


// TODO: implement compile-error test(s) -- perhaps with `trybuild`?
//
// #[test]
// fn test_table_derive_macro_missing_id() {
//     use squail_macros::Table;
// 
//     /// An example struct without an explicit id.
//     /// Should not compile and give a proper error message.
//     #[derive(Table)]
//     struct ShouldntWork {
//         data: i64, // missing `id: Option<i64>`
//     }
// }

