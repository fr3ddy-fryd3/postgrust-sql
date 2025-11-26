use crate::types::DataType;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
    },
    DropTable {
        name: String,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<crate::types::Value>,
    },
    Select {
        columns: Vec<SelectColumn>,
        from: String,
        joins: Vec<JoinClause>,
        filter: Option<Condition>,
        group_by: Option<Vec<String>>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
    },
    Update {
        table: String,
        assignments: Vec<(String, crate::types::Value)>,
        filter: Option<Condition>,
    },
    Delete {
        from: String,
        filter: Option<Condition>,
    },
    Begin,
    Commit,
    Rollback,
    ShowTables,
    // User management
    CreateUser {
        username: String,
        password: String,
        is_superuser: bool,
    },
    DropUser {
        username: String,
    },
    AlterUser {
        username: String,
        password: String,
    },
    // Database management
    CreateDatabase {
        name: String,
        owner: Option<String>,
    },
    DropDatabase {
        name: String,
    },
    // Privileges
    Grant {
        privilege: PrivilegeType,
        on_database: String,
        to_user: String,
    },
    Revoke {
        privilege: PrivilegeType,
        on_database: String,
        from_user: String,
    },
    // Metadata queries
    ShowUsers,
    ShowDatabases,
    // Enum types
    CreateType {
        name: String,
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrivilegeType {
    Connect,
    Create,
    Select,
    Insert,
    Update,
    Delete,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub foreign_key: Option<crate::types::ForeignKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Equals(String, crate::types::Value),
    NotEquals(String, crate::types::Value),
    GreaterThan(String, crate::types::Value),
    LessThan(String, crate::types::Value),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectColumn {
    Regular(String),              // Regular column name or *
    Aggregate(AggregateFunction), // Aggregate function
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggregateFunction {
    Count(CountTarget),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CountTarget {
    All,           // COUNT(*)
    Column(String), // COUNT(column)
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub on_left: String,  // left_table.column
    pub on_right: String, // right_table.column
}
