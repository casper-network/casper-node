use casper_sdk::schema::Schema;
use codegen::Scope;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
#[derive(Deserialize, Serialize)]
pub struct Codegen(Schema);

impl Codegen {
    pub fn new(schema: Schema) -> Self {
        Self(schema)
    }

    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let schema: Schema = serde_json::from_reader(file)?;
        Ok(Self(schema))
    }

    pub fn gen(&self) -> String {
        let mut scope = Scope::new();

        for (decl, def) in self.0.definitions.iter() {
            scope.new_struct(decl);
        }

        scope.to_string()
        // scope.new_struct(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let schema = Codegen::from_file("/tmp/cep18_schema.json").unwrap();
        eprintln!("{}", schema.gen());
    }
}
