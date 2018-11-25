extern crate proc_macro;
extern crate proc_macro2;

use graphql_parser::{parse_schema, query::Name, schema::*};
use heck::SnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    punctuated::Punctuated, token::Colon2, AngleBracketedGenericArguments, Ident, Path,
    PathArguments, PathSegment, Token,
};

#[macro_use]
mod macros;

#[proc_macro]
pub fn graphql_schema_from_file(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: TokenStream = input.into();

    let file = input.to_string().replace("\"", "");
    let pwd = std::env::current_dir().unwrap();
    let path = pwd.join(file);

    match read_file(&path) {
        Ok(schema) => parse_and_gen_schema(schema),
        Err(err) => panic!("{}", err),
    }
}

#[proc_macro]
pub fn graphql_schema(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: TokenStream = input.into();
    let schema = input.to_string();
    parse_and_gen_schema(schema)
}

fn parse_and_gen_schema(schema: String) -> proc_macro::TokenStream {
    let mut output = Output::new();

    match parse_schema(&schema) {
        Ok(doc) => gen_doc(doc, &mut output),
        Err(parse_error) => panic!("{}", parse_error),
    };

    output.tokens().into_iter().collect::<TokenStream>().into()
}

struct Output {
    tokens: Vec<TokenStream>,
    date_time_scalar_defined: bool,
    date_scalar_defined: bool,
}

impl Output {
    fn new() -> Self {
        Output {
            tokens: vec![],
            date_scalar_defined: false,
            date_time_scalar_defined: false,
        }
    }

    fn tokens(self) -> Vec<TokenStream> {
        self.tokens
    }

    fn push(&mut self, toks: TokenStream) {
        self.tokens.push(toks);
    }

    fn is_date_time_scalar_defined(&self) -> bool {
        self.date_time_scalar_defined
    }

    fn is_date_scalar_defined(&self) -> bool {
        self.date_scalar_defined
    }

    fn date_time_scalar_defined(&mut self) {
        self.date_time_scalar_defined = true
    }

    fn date_scalar_defined(&mut self) {
        self.date_scalar_defined = true
    }

    fn clone_without_tokens(&self) -> Self {
        Output {
            tokens: vec![],
            date_scalar_defined: self.date_scalar_defined,
            date_time_scalar_defined: self.date_time_scalar_defined,
        }
    }
}

fn gen_doc(doc: Document, out: &mut Output) {
    for def in doc.definitions {
        gen_def(def, out);
    }
}

fn gen_def(def: Definition, out: &mut Output) {
    use graphql_parser::schema::Definition::*;

    match def {
        DirectiveDefinition(_) => todo!(),
        SchemaDefinition(schema_def) => gen_schema_def(schema_def, out),
        TypeDefinition(type_def) => gen_type_def(type_def, out),
        TypeExtension(_) => todo!(),
    }
}

fn gen_schema_def(schema_def: SchemaDefinition, out: &mut Output) {
    // TODO: use
    //   position
    //   directives
    //   subscription

    let query = match schema_def.query {
        Some(query) => ident(query),
        None => panic!("Juniper requires that the schema type has a query"),
    };

    let mutation = match schema_def.mutation {
        Some(mutation) => quote_ident(mutation),
        None => quote! { juniper::EmptyMutation<()> },
    };

    (quote!{
        pub type Schema = juniper::RootNode<'static, #query, #mutation>;
    })
    .add_to(out)
}

fn gen_type_def(type_def: TypeDefinition, out: &mut Output) {
    use graphql_parser::schema::TypeDefinition::*;

    match type_def {
        Enum(_) => todo!(),
        InputObject(_) => todo!(),
        Interface(_) => todo!(),
        Object(obj_type) => gen_obj_type(obj_type, out),
        Scalar(scalar_type) => gen_scalar_type(scalar_type, out),
        Union(_) => todo!(),
    }
}

fn gen_scalar_type(scalar_type: ScalarType, out: &mut Output) {
    // TODO: use
    //   position
    //   description
    //   directives

    match &*scalar_type.name {
        "Date" => out.date_scalar_defined(),
        "DateTime" => out.date_time_scalar_defined(),
        _ => panic!("Only Date and DateTime scalars are supported at the moment"),
    };
}

fn gen_obj_type(obj_type: ObjectType, out: &mut Output) {
    // TODO: Use
    //   description
    //   implements_interface
    //   directives

    let name = ident(obj_type.name);

    let fields = gen_with(gen_field, obj_type.fields, out);

    quote! (
        juniper::graphql_object!(#name: Context |&self| {
            #fields
        });
    )
    .add_to(out)
}

fn gen_field(field: Field, out: &mut Output) {
    // TODO: Use
    //   description
    //   directives

    let name = ident(field.name);

    let field_type = gen_field_type(field.field_type, out);

    let field_method = ident(format!("field_{}", name.to_string().to_snake_case()));

    let args_names_and_types = field
        .arguments
        .into_iter()
        .map(|x| argument_to_name_and_rust_type(x, out))
        .collect::<Vec<_>>();

    let args = args_names_and_types
        .iter()
        .map(|(arg, arg_type)| {
            let arg = ident(arg);
            quote! { #arg: #arg_type, }
        })
        .collect::<TokenStream>();

    let params = args_names_and_types
        .iter()
        .map(|(arg, _)| {
            let arg = ident(arg);
            quote! { #arg, }
        })
        .collect::<TokenStream>();

    (quote! {
        field #name(&executor, #args) -> juniper::FieldResult<#field_type> {
            self.#field_method(&executor, #params)
        }
    })
    .add_to(out)
}

fn argument_to_name_and_rust_type(arg: InputValue, out: &Output) -> (Name, TokenStream) {
    // TODO: use
    //   position
    //   description
    //   default_value
    //   directives

    let arg_name = arg.name.to_snake_case();
    let arg_type = gen_field_type(arg.value_type, out);
    (arg_name, arg_type)
}

fn gen_field_type(field_type: Type, out: &Output) -> TokenStream {
    let field_type = TypeWithNullability::from_root_type(field_type);
    gen_field_type_with_nullability(field_type, out)
}

#[derive(Debug)]
enum Nullability {
    NotNull,
    Nullable,
}

#[derive(Debug)]
enum TypeWithNullability {
    NamedType(Name, Nullability),
    ListType(Box<TypeWithNullability>, Nullability),
}

impl TypeWithNullability {
    fn from_root_type(field_type: Type) -> Self {
        use self::Nullability::*;
        use graphql_parser::query::Type::*;

        match field_type {
            NamedType(name) => TypeWithNullability::NamedType(name, Nullable),
            ListType(inner) => Self::from_inner_type(*inner, Nullable),
            NonNullType(inner) => Self::from_inner_type(*inner, NotNull),
        }
    }

    fn from_inner_type(field_type: Type, nullability: Nullability) -> Self {
        use graphql_parser::query::Type::*;

        match field_type {
            NamedType(name) => TypeWithNullability::NamedType(name, nullability),
            ListType(inner) => {
                let inner = Self::from_root_type(*inner);
                TypeWithNullability::ListType(Box::new(inner), nullability)
            }
            NonNullType(inner) => Self::from_inner_type(*inner, nullability),
        }
    }
}

fn gen_field_type_with_nullability(field_type: TypeWithNullability, out: &Output) -> TokenStream {
    use self::{Nullability::*, TypeWithNullability::*};

    match field_type {
        NamedType(name, nullability) => {
            let name = graphql_scalar_type_to_rust_type(name, out);
            match nullability {
                NotNull => quote! { #name },
                Nullable => quote! { Option<#name> },
            }
        }
        ListType(inner, nullability) => {
            let inner = gen_field_type_with_nullability(*inner, out);
            match nullability {
                NotNull => quote! { Vec<#inner> },
                Nullable => quote! { Option<Vec<#inner>> },
            }
        }
    }
}

// Type according to https://graphql.org/learn/schema/#scalar-types
fn graphql_scalar_type_to_rust_type(name: Name, out: &Output) -> TokenStream {
    match &*name {
        "Int" => quote! { i32 },
        "Float" => quote! { f64 },
        "String" => quote! { String },
        "Boolean" => quote! { bool },
        "ID" => todo!(),
        "Date" => {
            if out.is_date_scalar_defined() {
                quote! { chrono::naive::NaiveDate }
            } else {
                panic!(
                    "Fields with type `Date` is only allowed if you have define a scalar named `Date`"
                )
            }
        }
        "DateTime" => {
            if out.is_date_scalar_defined() {
                quote! { chrono::DateTime<chrono::offset::Utc> }
            } else {
                panic!(
                    "Fields with type `DateTime` is only allowed if you have define a scalar named `DateTime`"
                )
            }
        }
        name => quote_ident(name),
    }
}

fn push_simple_path(s: &str, segments: &mut Punctuated<PathSegment, Colon2>) {
    segments.push(PathSegment {
        ident: ident(s),
        arguments: PathArguments::None,
    });
}

trait AddToOutput {
    fn add_to(self, out: &mut Output);
}

impl AddToOutput for TokenStream {
    fn add_to(self, out: &mut Output) {
        out.push(self);
    }
}

fn ident<T: AsRef<str>>(name: T) -> Ident {
    Ident::new(name.as_ref(), Span::call_site())
}

fn quote_ident<T: AsRef<str>>(name: T) -> TokenStream {
    let ident = ident(name);
    quote! { #ident }
}

fn gen_with<F, T>(f: F, ts: Vec<T>, other: &Output) -> TokenStream
where
    F: Fn(T, &mut Output),
{
    let mut acc = other.clone_without_tokens();
    for t in ts {
        f(t, &mut acc);
    }
    acc.tokens().into_iter().collect::<TokenStream>()
}

fn read_file(path: &std::path::PathBuf) -> Result<String, std::io::Error> {
    use std::{fs::File, io::prelude::*};
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}