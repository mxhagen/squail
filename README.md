
# <img width="48px" height="48px" src="./icon.svg"/> squail


Simple generated functions to use Rust structs as Sqlite tables with minimal manual Sqlite.

---

## Why

> Just reading [Example code](#example) should suffice to explain why to use this over manually written `rusqlite` wrappers.

The crate `rusqlite` provides pretty nice Sqlite bindings that let you interact with Sqlite from rust.

But in order to simply use Rust structs to represent table rows, you always need to manually write the *mostly-equivalent-and-always-tedious-to-maintain* Sqlite statements for those structs.

Since these wrapper functions (eg. `insert(my_struct, &connection)`) mostly follow the exact same structure, and in most cases **have to** change whenever the struct definition changes, we can just generate them based on the struct itself. This not only makes it faster to get something working, but significantly speeds up changes down the line, by eliminating the need to change Sqlite statements manually whenever the data structure changes.

The `#[derive(Table)]` macro that squail provides does just this &ndash; generating the most commonly used wrappers to interact with a table corresponding to a struct.


## Example

```rust
use rusqlite::Connection;
use squail::Table;

#[derive(Table)] // <-- This is where the magic happens
struct Person {
    id: Option<i64>, // Required for any table: Sqlite rowid
    name: String,
    age: i32,
    
    // NOTE: All fields need to implement rusqlite::types::{FromSql, ToSql}.
    //       If not implemented by rusqlite, you can implement these yourself.
}

fn main() {
    // Connect to the database as usual
    let conn = Connection::open_in_memory().unwrap();
    
    // Create a `Person` table according to the struct.
    // --> columns: id, name, age
    Person::create_table(&conn).unwrap();
    
    // Create some person
    let mut larry = Person {
        id: None,
        name: "larry".into(),
        age: 24,
    };
    
    // Insert larry into the `Person` table.
    // This automatically sets larry.id to `Some(last_insert_rowid())`
    larry.insert(&conn).unwrap();
    let larry_id = larry.id.unwrap();
    
    // Query the database, using the now set id.
    let larry_copy = Person::get_by_id(&conn, larry_id).unwrap();
    assert_eq!(larry_copy, Some(larry_copy));

    // Delete larry from the table.
    // This sets the id back to None -- since the row is deleted.
    let deleted_something = larry.delete(&conn).unwrap();
    assert!(deleted_something); // delete returns a wrapped boolean

    // Trying to retrieve a deleted Person returns None
    let deleted_larry = Person::get_by_id(&conn, larry_id).unwrap();
    assert_eq!(deleted_larry, None);
}
```
