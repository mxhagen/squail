use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

// TODO: wrap functions in a trait

// TODO: doc comments

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

    let mut to_sql_trait_bounds = Vec::new();
    let mut from_sql_trait_bounds = Vec::new();

    for field in fields.named.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        field_names.push(field_name);
        field_getters.push(quote!(#field_name: row.get(stringify!(#field_name))?));
        field_accessors.push(quote!(self.#field_name));

        to_sql_trait_bounds.push(quote!(#field_type: rusqlite::types::ToSql));
        from_sql_trait_bounds.push(quote!(#field_type: rusqlite::types::FromSql));

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

    if !field_names.iter().map(|id| id.to_string()).any(|id| &id == "id") {
        panic!("Structs annotated with `Table` require a primary key field `id: Option<i64>`.");
    }


    let create_table_sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (id INTEGER PRIMARY KEY AUTOINCREMENT, {});",
        table_name,
        column_names.join(", ")
    );

    let create_table_fn = quote! {
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
        pub fn insert(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<i64>
            where #(#to_sql_trait_bounds),*
        {
            conn.execute(#insert_sql, rusqlite::params![#(#field_accessors),*])?;
            let id = conn.last_insert_rowid();
            self.id = Some(id);
            Ok(id)
        }
    };


    let upsert_fn = quote! {
        pub fn upsert(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<i64>
            where #(#to_sql_trait_bounds),*
        {
            match self.id {
                None => self.insert(conn),
                Some(id) => {
                    if !self.update(conn)? {
                        return self.insert(conn);
                    }
                    Ok(id)
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
        pub fn update(&self, conn: &rusqlite::Connection) -> rusqlite::Result<bool>
            where #(#to_sql_trait_bounds),*
        {
            if self.id.is_none() {
                return Ok(false);
            }
            let updated_count = conn.execute(#update_sql, rusqlite::params![#(#field_accessors),*])?;
            Ok(updated_count > 0)
        }
    };


    let sync_fn = quote! {
        pub fn sync(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<bool>
            where #(#from_sql_trait_bounds),*
        {
            if self.id.is_none() {
                return Ok(false);
            }
            match #struct_name::get_by_id(conn, self.id.unwrap())? {
                Some(person) => *self = person,
                _ => return Ok(false),
            };
            Ok(true)
        }
    };


    let get_by_id_fn = quote! {
        pub fn get_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Option<Self>>
        where
            Self: Sized,
            #(#from_sql_trait_bounds),*
        {
            let mut stmt = conn.prepare(&format!("SELECT * FROM {} WHERE id = ?", #table_name))?;
            let mut rows = stmt.query(rusqlite::params![id])?;

            if let Some(row) = rows.next()? {
                Ok(Some(Self { #(#field_getters),* }))
            } else {
                Ok(None)
            }
        }
    };


    let delete_fn = quote! {
        pub fn delete(&mut self, conn: &rusqlite::Connection) -> rusqlite::Result<bool>
            where #(#to_sql_trait_bounds),*
        {
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
        pub fn delete_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<()> {
            conn.execute(&format!(
                    "DELETE FROM {} WHERE id = ?",
                    #table_name
            ), rusqlite::params![id])?;
            Ok(())
        }
    };


    let drop_table_fn = quote! {
        pub fn drop_table(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
            conn.execute(&format!("DROP TABLE {}", #table_name), [])?;
            Ok(())
        }
    };


    let expanded = quote! {
        impl #struct_name {
            #create_table_fn
            #insert_fn
            #upsert_fn
            #update_fn
            #sync_fn
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
