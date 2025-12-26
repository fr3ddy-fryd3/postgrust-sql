use crate::parser::Statement;
use crate::types::Value;
use std::collections::HashMap;

/// Prepared statement cache entry (v2.4.0 - Extended Query Protocol)
#[derive(Clone)]
pub struct PreparedStatement {
    pub query: String,
    pub statement: Option<Statement>,
    pub param_types: Vec<i32>,
}

/// Portal - bound prepared statement with parameters (v2.4.0 - Extended Query Protocol)
#[derive(Clone)]
pub struct Portal {
    pub statement_name: String,
    pub param_values: Vec<Option<Value>>,
}

/// Cache for prepared statements and portals (v2.4.0 - Extended Query Protocol)
#[derive(Default)]
pub struct PreparedStatementCache {
    statements: HashMap<String, PreparedStatement>,
    portals: HashMap<String, Portal>,
}

impl PreparedStatementCache {
    pub fn new() -> Self {
        Self {
            statements: HashMap::new(),
            portals: HashMap::new(),
        }
    }

    /// Store a prepared statement
    pub fn add_statement(&mut self, name: String, query: String, param_types: Vec<i32>) {
        self.statements.insert(
            name,
            PreparedStatement {
                query,
                statement: None,
                param_types,
            },
        );
    }

    /// Get a prepared statement by name
    pub fn get_statement(&self, name: &str) -> Option<&PreparedStatement> {
        self.statements.get(name)
    }

    /// Get a mutable prepared statement by name
    pub fn get_statement_mut(&mut self, name: &str) -> Option<&mut PreparedStatement> {
        self.statements.get_mut(name)
    }

    /// Remove a prepared statement
    pub fn remove_statement(&mut self, name: &str) -> bool {
        self.statements.remove(name).is_some()
    }

    /// Store a portal (bound statement with parameters)
    pub fn add_portal(
        &mut self,
        portal_name: String,
        statement_name: String,
        param_values: Vec<Option<Value>>,
    ) {
        self.portals.insert(
            portal_name,
            Portal {
                statement_name,
                param_values,
            },
        );
    }

    /// Get a portal by name
    pub fn get_portal(&self, name: &str) -> Option<&Portal> {
        self.portals.get(name)
    }

    /// Remove a portal
    pub fn remove_portal(&mut self, name: &str) -> bool {
        self.portals.remove(name).is_some()
    }

    /// Clear all statements and portals
    pub fn clear(&mut self) {
        self.statements.clear();
        self.portals.clear();
    }
}

/// Substitute parameters in SQL query ($1, $2, ...) with actual values (v2.4.0)
pub fn substitute_parameters(query: &str, params: &[Option<Value>]) -> String {
    let mut result = query.to_string();

    // Replace $1, $2, ... with actual values
    for (i, param) in params.iter().enumerate() {
        let placeholder = format!("${}", i + 1);

        let value_str = match param {
            None => "NULL".to_string(),
            Some(Value::Integer(n)) => n.to_string(),
            Some(Value::SmallInt(n)) => n.to_string(),
            Some(Value::Real(f)) => f.to_string(),
            Some(Value::Numeric(d)) => d.to_string(),
            Some(Value::Text(s)) | Some(Value::Char(s)) => {
                format!("'{}'", s.replace('\'', "''")) // Escape single quotes
            }
            Some(Value::Boolean(b)) => {
                if *b {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
            Some(Value::Date(d)) => format!("'{}'", d.format("%Y-%m-%d")),
            Some(Value::Timestamp(ts)) => format!("'{}'", ts.format("%Y-%m-%d %H:%M:%S")),
            Some(Value::TimestampTz(ts)) => format!("'{}'", ts.format("%Y-%m-%d %H:%M:%S%z")),
            Some(Value::Uuid(u)) => format!("'{u}'"),
            Some(Value::Json(j)) => format!("'{}'", j.replace('\'', "''")),
            Some(Value::Bytea(b)) => {
                // Convert to PostgreSQL hex format: \x followed by hex bytes
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("'\\x{hex}'")
            }
            Some(Value::Enum(_, v)) => format!("'{v}'"),
            Some(Value::Null) => "NULL".to_string(),
        };

        result = result.replace(&placeholder, &value_str);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepared_statement_cache() {
        let mut cache = PreparedStatementCache::new();

        // Add statement
        cache.add_statement(
            "stmt1".to_string(),
            "SELECT * FROM users WHERE id = $1".to_string(),
            vec![23], // OID for INTEGER
        );

        // Get statement
        let stmt = cache.get_statement("stmt1");
        assert!(stmt.is_some());
        assert_eq!(stmt.unwrap().query, "SELECT * FROM users WHERE id = $1");

        // Remove statement
        assert!(cache.remove_statement("stmt1"));
        assert!(cache.get_statement("stmt1").is_none());
    }

    #[test]
    fn test_portal_cache() {
        let mut cache = PreparedStatementCache::new();

        // Add portal
        cache.add_portal(
            "portal1".to_string(),
            "stmt1".to_string(),
            vec![Some(Value::Integer(42))],
        );

        // Get portal
        let portal = cache.get_portal("portal1");
        assert!(portal.is_some());
        assert_eq!(portal.unwrap().statement_name, "stmt1");

        // Remove portal
        assert!(cache.remove_portal("portal1"));
        assert!(cache.get_portal("portal1").is_none());
    }

    #[test]
    fn test_substitute_parameters() {
        let query = "SELECT * FROM users WHERE id = $1 AND name = $2";

        let params = vec![Some(Value::Integer(42)), Some(Value::Text("Alice".to_string()))];

        let result = substitute_parameters(query, &params);
        assert_eq!(result, "SELECT * FROM users WHERE id = 42 AND name = 'Alice'");
    }

    #[test]
    fn test_substitute_parameters_with_null() {
        let query = "UPDATE users SET email = $1 WHERE id = $2";

        let params = vec![None, Some(Value::Integer(42))];

        let result = substitute_parameters(query, &params);
        assert_eq!(result, "UPDATE users SET email = NULL WHERE id = 42");
    }

    #[test]
    fn test_substitute_parameters_escape_quotes() {
        let query = "INSERT INTO users (name) VALUES ($1)";

        let params = vec![Some(Value::Text("O'Brien".to_string()))];

        let result = substitute_parameters(query, &params);
        assert_eq!(result, "INSERT INTO users (name) VALUES ('O''Brien')");
    }
}
