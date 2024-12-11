use std::collections::HashMap;
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

// TODO: wrap functions in a trait? would probably use the other (main) crate

// TODO: own error enum (for update). also use the other (main) crate for this


#[proc_macro_derive(Table)]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = input.ident;
    let table_name = struct_name.to_string();

    let fields = if let Data::Struct(data) = input.data {
        if let Fields::Named(fields) = data.fields {
            fields
        } else {
            panic!("#[derive(Table)] only supports structs with named fields");
        }
    } else {
        panic!("#[derive(Table)] only supports structs");
    };

    let mut column_names = Vec::new();

    let mut field_names = Vec::new();
    let mut field_getters = Vec::new();
    let mut field_accessors = Vec::new();

    let mut to_sql_trait_bounds = HashMap::new();
    let mut from_sql_trait_bounds = HashMap::new();

    for field in fields.named.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        field_names.push(field_name);
        field_getters.push(quote!(#field_name: row.get(stringify!(#field_name))?));
        field_accessors.push(quote!(self.#field_name));

        to_sql_trait_bounds.insert(stringify!(#field_type), quote!(#field_type: rusqlite::types::ToSql));
        from_sql_trait_bounds.insert(stringify!(#field_type), quote!(#field_type: rusqlite::types::FromSql));

        if field_name == "id" {
            if let syn::Type::Path(type_path) = field_type {
                let segment = &type_path.path.segments.first().unwrap();

                // TODO: not sure if .trim().eq() always works as intended here -- check
                if segment.ident != "Option" || !segment.arguments.to_token_stream().to_string().trim().eq("< i64 >") {
                    panic!("The `id` field must be of type `Option<i64>`");
                }
            } else {
                panic!("The `id` field must be of type `Option<i64>`");
            }
        } else {
            column_names.push(field_name.to_string());
        }
    }

    let to_sql_trait_bounds = to_sql_trait_bounds.values().collect::<Vec<_>>();
    let from_sql_trait_bounds = from_sql_trait_bounds.values().collect::<Vec<_>>();

    if !field_names.iter().map(|id| id.to_string()).any(|id| &id == "id") {
        panic!("Structs annotated with `Table` require a primary key field `id: Option<i64>`.");
    }


    let create_table_sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (id INTEGER PRIMARY KEY AUTOINCREMENT, {});",
        table_name,
        column_names.join(", ")
    );

    let create_table_fn = quote! {
        /// Use a `Connection` to create a table named after the struct (`#struct_name`)
        /// If the table already exists, this returns `Ok(())` and does nothing.
        pub fn create_table(conn: &rusqlite::Connection) -> rusqlite::Result<()>
            where #(#to_sql_trait_bounds),*
        {
            conn.execute(#create_table_sql, [])?;
            Ok(())
        }
    };


    let insert_sql = format!(
        "INSERT INTO {} (id, {}) VALUES ({});",
        table_name,
        column_names.join(", "),
        vec!["?"; field_names.len()].join(", ")
    );

    let insert_fn = quote! {
        /// Insert struct instance into the table, setting `self.id` to
        /// `Some(last_insert_rowid())` if it was `None`.
        pub fn insert(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<i64>
            where #(#to_sql_trait_bounds),*
        {
            conn.execute(#insert_sql, rusqlite::params![#(#field_accessors),*])?;
            // TODO: test this with manually set id. also test that this can't update!!!
            let id = conn.last_insert_rowid();
            self.id = Some(id);
            Ok(id)
        }
    };


    let update_or_insert_fn = quote! {
        /// Update a table row using the calling struct instance.
        /// If the row does not yet exist, it is inserted into the table.
        pub fn update_or_insert(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<i64>
            where #(#to_sql_trait_bounds),*
        {
            match self.id {
                None => self.insert(conn),
                Some(id) => {
                    match self.update(conn) {
                        Ok(_) => return Ok(id),
                        Err(_) => return self.insert(conn),
                    }
                },
            }
        }
    };


    let update_sql = format!(
        "UPDATE OR IGNORE {} SET ({}) = ({}) WHERE id = ?1",
        table_name,
        field_names.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", "),
        (1..=field_names.len()).map(|i| format!("?{}", i)).collect::<Vec<_>>().join(", "),
    );

    let update_fn = quote! {
        /// Update a table row using the calling struct instance.
        ///
        /// If `id` is `None`, this fails with `InvalidQuery`.
        /// If the row does not exist, this fails with `QueryReturnedNoRows`.
        ///
        /// A version that inserts a new row instead also exists. See `update_or_insert`.
        pub fn update(&self, conn: &rusqlite::Connection) -> rusqlite::Result<()>
            where #(#to_sql_trait_bounds),*
        {
            if self.id.is_none() {
                return Err(rusqlite::Error::InvalidQuery)
            }
            let updated_count = conn.execute(#update_sql, rusqlite::params![#(#field_accessors),*])?;
            match updated_count {
                0 => Err(rusqlite::Error::QueryReturnedNoRows),
                _ => Ok(()),
            }
        }
    };


    let sync_fn = quote! {
        /// Sync a struct instance with the database state.
        /// This "updates" the structs fields using its database entry.
        ///
        /// Result contains `false` if `self.id == None` or if no row with that `id` was found.
        ///
        /// To update database entry using the structs fields, see `update`.
        pub fn sync(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<bool>
            where #(#from_sql_trait_bounds),*
        {
            if self.id.is_none() {
                return Ok(false);
            }
            match #struct_name::get_by_id(conn, self.id.unwrap()) {
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
                Ok(person) => {
                    *self = person;
                    Ok(true)
                },
                Err(e) => Err(e),
            }
        }
    };


    let from_sql_row_fn = quote! {
        /// Convert a `rusqlite::Row` received through a query to an instance of the struct
        pub fn from_sql_row(row: &rusqlite::Row) -> rusqlite::Result<Self>
        where
            Self: Sized,
            #(#from_sql_trait_bounds),*
        {
            Ok(Self { #(#field_getters),* })
        }
    };


    let get_by_id_fn = quote! {
        /// Get a person from the table using their `id` (corresponding to the sqlite rowid)
        pub fn get_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Self>
        where
            Self: Sized,
            #(#from_sql_trait_bounds),*
        {
            let mut stmt = conn.prepare(&format!("SELECT * FROM {} WHERE id = ?", #table_name))?;
            let mut rows = stmt.query(rusqlite::params![id])?;

            if let Some(row) = rows.next()? {
                Self::from_sql_row(row)
            } else {
                Err(rusqlite::Error::QueryReturnedNoRows)
            }
        }
    };


    let delete_fn = quote! {
        /// Delete row corresponding to the struct instance from the database.
        /// Deletes the entry with rowid equal to `self.id` without further checks.
        ///
        /// Result contains `true` if a row was deleted.
        pub fn delete(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<bool> {
            if self.id.is_none() {
                return Ok(false);
            }
            let updated_count = conn.execute(&format!(
                    "DELETE FROM {} WHERE id = ?",
                    #table_name
            ), rusqlite::params![self.id])?;
            self.id = None;
            Ok(updated_count > 0)
        }
    };

    let delete_by_id_fn = quote! {
        /// Delete a row from the database by rowid.
        ///
        /// Result contains `true` if a row was deleted.
        pub fn delete_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<()> {
            conn.execute(&format!(
                    "DELETE FROM {} WHERE id = ?",
                    #table_name
            ), rusqlite::params![id])?;
            Ok(())
        }
    };


    let drop_table_fn = quote! {
        /// Use a `Connection` to drop the table named after the struct (`#struct_name`)
        pub fn drop_table(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
            conn.execute(&format!("DROP TABLE {}", #table_name), [])?;
            Ok(())
        }
    };


    let expanded = quote! {
        impl #struct_name {
            #create_table_fn
            #insert_fn
            #update_or_insert_fn
            #update_fn
            #sync_fn
            #from_sql_row_fn
            #get_by_id_fn
            #delete_fn
            #delete_by_id_fn
            #drop_table_fn
        }
    };

    // if you want to see the generated code:
    // println!("{}", expanded.to_string());
    TokenStream::from(expanded)
}
