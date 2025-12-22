use crate::types::DataType;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
        owner: Option<String>,  // v2.3.0: Table owner
    },
    DropTable {
        name: String,
    },
    AlterTable {
        name: String,
        operation: AlterTableOperation,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<crate::types::Value>,
    },
    Select {
        distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        joins: Vec<JoinClause>,
        filter: Option<Condition>,
        group_by: Option<Vec<String>>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
        offset: Option<usize>,
    },
    /// Set operations (v1.10.0)
    Union {
        left: Box<Statement>,
        right: Box<Statement>,
        all: bool,  // UNION ALL if true, UNION (DISTINCT) if false
    },
    Intersect {
        left: Box<Statement>,
        right: Box<Statement>,
    },
    Except {
        left: Box<Statement>,
        right: Box<Statement>,
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
    // Role management
    CreateRole {
        role_name: String,
        is_superuser: bool,
    },
    DropRole {
        role_name: String,
    },
    GrantRole {
        role_name: String,
        to_user: String,
    },
    RevokeRole {
        role_name: String,
        from_user: String,
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
    // Indexes
    CreateIndex {
        name: String,
        table: String,
        columns: Vec<String>,  // v1.9.0: supports composite indexes
        unique: bool,
        index_type: crate::index::IndexType,
    },
    DropIndex {
        name: String,
    },
    // MVCC cleanup
    Vacuum {
        table: Option<String>, // None = all tables
    },
    // Query analysis (v1.8.0)
    Explain {
        statement: Box<Statement>,
    },
    // Views (v1.10.0)
    CreateView {
        name: String,
        query: String,  // SQL query as string
    },
    DropView {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeType {
    Connect,
    Create,
    Select,
    Insert,
    Update,
    Delete,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub foreign_key: Option<crate::types::ForeignKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlterTableOperation {
    AddColumn(ColumnDef),
    DropColumn(String),
    RenameColumn { old_name: String, new_name: String },
    RenameTable(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Equals(String, crate::types::Value),
    NotEquals(String, crate::types::Value),
    GreaterThan(String, crate::types::Value),
    LessThan(String, crate::types::Value),
    GreaterThanOrEqual(String, crate::types::Value),  // v1.8.0
    LessThanOrEqual(String, crate::types::Value),     // v1.8.0
    Between(String, crate::types::Value, crate::types::Value), // v1.8.0: col BETWEEN a AND b
    Like(String, String),                              // v1.8.0: col LIKE pattern
    In(String, Vec<crate::types::Value>),             // v1.8.0: col IN (list)
    IsNull(String),                                    // v1.8.0: col IS NULL
    IsNotNull(String),                                 // v1.8.0: col IS NOT NULL
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// CASE expression (v1.10.0)
#[derive(Debug, Clone, PartialEq)]
pub struct CaseExpression {
    pub when_clauses: Vec<WhenClause>,      // WHEN conditions
    pub else_value: Option<crate::types::Value>, // ELSE value (optional)
    pub alias: Option<String>,               // AS alias (optional)
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhenClause {
    pub condition: Condition,                // WHEN condition
    pub result: crate::types::Value,         // THEN result
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectColumn {
    Regular(String),              // Regular column name or *
    Aggregate(AggregateFunction), // Aggregate function
    Case(CaseExpression),         // CASE expression (v1.10.0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateFunction {
    Count(CountTarget),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountTarget {
    All,           // COUNT(*)
    Column(String), // COUNT(column)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub on_left: String,  // left_table.column
    pub on_right: String, // right_table.column
}
