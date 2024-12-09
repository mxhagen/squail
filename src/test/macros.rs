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
    let larry_id = larry.id.expect("After (mutable) insertion, id should not be None");

    larry.age += 1;
    let updated_something = larry.update(&conn).expect("Updating should work");
    assert!(updated_something, "Should have updated a row");

    let larry_copy = Person::get_by_id(&conn, larry_id).expect("Querying a row should work");
    assert_eq!(larry_copy, Some(larry.clone()), "Retrieving inserted row should give an identical row");

    let deleted_something = larry.delete(&conn).expect("Deletion should work");
    // also works: `Person::delete_by_id(&conn, larry_id).unwrap();`
    assert!(deleted_something, "Should have deleted something");

    let deleted_larry = Person::get_by_id(&conn, larry_id).expect("Querying a deleted row should return Ok(None), not Err(_)");
    assert_eq!(deleted_larry, None, "Received row that should have been deleted");

    let id = larry.upsert(&conn).expect("Upsertion (insert) should work");
    let larry_id = larry.id.expect("After (mutable) upsertion, id should not be None");

    let larry_copy = Person::get_by_id(&conn, larry_id).expect("Querying a row should work");
    assert_eq!(id, larry_id, "Upsert should return correct id");
    assert_eq!(larry_copy, Some(larry.clone()), "Retrieving upserted row should give an identical row");

    larry.age += 1;
    let id = larry.upsert(&conn).expect("Upsertion (update) should work");
    let larry_id = larry.id.expect("After (mutable) upsertion, id should not be None");
    assert_eq!(id, larry_id, "Upsert should return correct id");

    let larry_copy = Person::get_by_id(&conn, larry_id).expect("Querying a row should work");
    assert_eq!(larry_copy, Some(larry.clone()), "Retrieving upserted row should give an identical row");

    conn.execute("UPDATE Person SET (age) = (27) WHERE id = ?1", [larry_id])
        .expect("Explicit Sqlite statement (not a library test) failed");

    let found = larry.sync(&conn).expect("Syncing struct to existing row should succeed");
    assert!(found, "Row should have been found");
    assert_eq!(larry.age, 27, "Syncing struct to edited table row should work");

    Person::drop_table(&conn).expect("Dropping table should work");
    Person::drop_table(&conn).expect_err("Dropping previously dropped table should err");

    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='Person');", [], |row| row.get(0)).expect("Explicit Sqlite statement (not a library test) failed");
    assert!(!exists, "Deleted table should not exist anymore but does");
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

