# RustDB Data Types - Current vs PostgreSQL

## Currently Implemented (5 types)

### 1. **Integer** (i64)
- **RustDB:** `INTEGER`, `INT`
- **PostgreSQL equivalent:** `BIGINT` (8 bytes, -9223372036854775808 to 9223372036854775807)
- ✅ **Status:** Fully working

### 2. **Real** (f64)
- **RustDB:** `REAL`, `FLOAT`
- **PostgreSQL equivalent:** `DOUBLE PRECISION` (8 bytes, 15 decimal digits precision)
- ✅ **Status:** Fully working

### 3. **Text** (String)
- **RustDB:** `TEXT`, `VARCHAR`
- **PostgreSQL equivalent:** `TEXT` (variable unlimited length)
- ✅ **Status:** Fully working
- **Note:** No length limit (PostgreSQL VARCHAR(n) not enforced)

### 4. **Boolean** (bool)
- **RustDB:** `BOOLEAN`, `BOOL`
- **PostgreSQL equivalent:** `BOOLEAN` (true/false/NULL)
- ✅ **Status:** Fully working

### 5. **Serial** (auto-increment i64)
- **RustDB:** `SERIAL`
- **PostgreSQL equivalent:** `SERIAL` (auto-incrementing integer, 1 to 2147483647)
- ✅ **Status:** Fully working
- **Note:** Our SERIAL is i64 (larger than PostgreSQL's i32 SERIAL)

---

## Missing PostgreSQL Data Types (Priority Order)

### 🔴 HIGH PRIORITY - Numeric Types

#### 1. **SMALLINT** (i16)
```sql
CREATE TABLE test (id SMALLINT);  -- -32768 to 32767
```
- **PostgreSQL:** 2 bytes
- **Use case:** Small numbers, age, quantity
- **Implementation:** Add `Value::SmallInt(i16)`, `DataType::SmallInt`

#### 2. **BIGSERIAL**
```sql
CREATE TABLE test (id BIGSERIAL);  -- Auto-increment, larger range
```
- **PostgreSQL:** Auto-incrementing BIGINT
- **Use case:** High-volume tables
- **Implementation:** Like SERIAL but for BIGINT range

#### 3. **NUMERIC/DECIMAL(precision, scale)**
```sql
CREATE TABLE products (price NUMERIC(10, 2));  -- 10 digits, 2 after decimal
```
- **PostgreSQL:** Arbitrary precision
- **Use case:** Money, exact calculations
- **Implementation:** Could use `rust_decimal` crate or store as string

### 🟡 MEDIUM PRIORITY - String Types

#### 4. **CHAR(n)** - Fixed length
```sql
CREATE TABLE test (code CHAR(5));  -- Fixed 5 characters
```
- **PostgreSQL:** Fixed-length, space-padded
- **Use case:** Fixed codes (country codes, etc.)
- **Implementation:** Store length in DataType, pad on insert

#### 5. **VARCHAR(n)** - Variable length with limit
```sql
CREATE TABLE users (username VARCHAR(50));
```
- **PostgreSQL:** Variable up to n characters
- **Currently:** Parsed but not enforced
- **Implementation:** Store max_length in DataType, validate on insert

### 🟡 MEDIUM PRIORITY - Date/Time Types

#### 6. **DATE**
```sql
CREATE TABLE events (event_date DATE);  -- '2025-01-15'
```
- **PostgreSQL:** Date only (no time)
- **Use case:** Birthdays, event dates
- **Implementation:** Use `chrono::NaiveDate`

#### 7. **TIME**
```sql
CREATE TABLE schedule (start_time TIME);  -- '14:30:00'
```
- **PostgreSQL:** Time only (no date)
- **Use case:** Daily schedules
- **Implementation:** Use `chrono::NaiveTime`

#### 8. **TIMESTAMP**
```sql
CREATE TABLE logs (created_at TIMESTAMP);  -- '2025-01-15 14:30:00'
```
- **PostgreSQL:** Date and time
- **Use case:** Logging, audit trails
- **Implementation:** Use `chrono::NaiveDateTime`

#### 9. **TIMESTAMPTZ** (with timezone)
```sql
CREATE TABLE logs (created_at TIMESTAMPTZ);
```
- **PostgreSQL:** Timestamp with timezone
- **Use case:** Global applications
- **Implementation:** Use `chrono::DateTime<Utc>`

### 🟢 LOW PRIORITY - Special Types

#### 10. **UUID**
```sql
CREATE TABLE users (id UUID);  -- '550e8400-e29b-41d4-a716-446655440000'
```
- **PostgreSQL:** 128-bit universal unique identifier
- **Use case:** Distributed systems, unique IDs
- **Implementation:** Use `uuid` crate

#### 11. **JSON/JSONB**
```sql
CREATE TABLE data (metadata JSON);
```
- **PostgreSQL:** JSON data
- **Use case:** Flexible schemas
- **Implementation:** Store as text, optionally validate

#### 12. **BYTEA** (binary data)
```sql
CREATE TABLE files (data BYTEA);
```
- **PostgreSQL:** Binary strings
- **Use case:** Files, images
- **Implementation:** `Value::Bytes(Vec<u8>)`

#### 13. **ARRAY types**
```sql
CREATE TABLE test (tags TEXT[]);  -- ['tag1', 'tag2']
```
- **PostgreSQL:** Arrays of any type
- **Use case:** Multiple values
- **Implementation:** `Value::Array(Vec<Value>)`

### 🔵 OPTIONAL - Advanced Types

#### 14. **ENUM**
```sql
CREATE TYPE mood AS ENUM ('happy', 'sad', 'neutral');
CREATE TABLE person (current_mood mood);
```
- **PostgreSQL:** User-defined enumerated type
- **Implementation:** New statement `CREATE TYPE`

#### 15. **Geometric types** (POINT, LINE, etc.)
#### 16. **Network types** (INET, CIDR, MACADDR)
#### 17. **Range types** (INT4RANGE, TSRANGE)
#### 18. **XML type**

---

## Recommended Implementation Plan

### Phase 1: Essential Numeric Types
1. ✅ **SERIAL** - Done!
2. **SMALLINT** - Easy, just add i16
3. **BIGSERIAL** - Similar to SERIAL
4. **NUMERIC(p,s)** - For money/precision

### Phase 2: Date/Time (Very Common)
5. **DATE** - Essential for most apps
6. **TIMESTAMP** - Essential for logging
7. **TIMESTAMPTZ** - Global apps

### Phase 3: String Constraints
8. **VARCHAR(n)** - Enforce length limits
9. **CHAR(n)** - Fixed length strings

### Phase 4: Modern Types
10. **UUID** - Very popular in modern apps
11. **JSON/JSONB** - Flexible data
12. **BYTEA** - Binary data

---

## Current Coverage vs PostgreSQL

**RustDB:** 5 data types
**PostgreSQL:** 40+ data types

**Coverage:** ~12% of core types

**Most critical missing:**
- ❌ DATE/TIME types (very common in real apps)
- ❌ NUMERIC/DECIMAL (money, precise calculations)
- ❌ SMALLINT (space efficiency)
- ❌ UUID (modern apps)

---

## Code Changes Needed (Example: SMALLINT)

### 1. Add to Value enum (`types.rs`)
```rust
pub enum Value {
    Null,
    SmallInt(i16),  // NEW
    Integer(i64),
    Real(f64),
    Text(String),
    Boolean(bool),
}
```

### 2. Add to DataType enum (`types.rs`)
```rust
pub enum DataType {
    SmallInt,  // NEW
    Integer,
    Real,
    Text,
    Boolean,
    Serial,
}
```

### 3. Add parser (`parser.rs`)
```rust
fn data_type(input: &str) -> IResult<&str, DataType> {
    alt((
        map(tag_no_case("SMALLINT"), |_| DataType::SmallInt),  // NEW
        map(tag_no_case("SERIAL"), |_| DataType::Serial),
        // ...
    ))(input)
}
```

### 4. Add value parser (`parser.rs`)
```rust
fn value(input: &str) -> IResult<&str, Value> {
    alt((
        map(tag_no_case("NULL"), |_| Value::Null),
        // Parse small integers with range check
        map_res(
            recognize(tuple((opt(char('-')), digit1))),
            |s: &str| {
                let num = s.parse::<i64>()?;
                if num >= i16::MIN as i64 && num <= i16::MAX as i64 {
                    Ok(Value::SmallInt(num as i16))
                } else {
                    Ok(Value::Integer(num))
                }
            }
        ),
        // ...
    ))(input)
}
```

### 5. Update Display (`types.rs`)
```rust
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::SmallInt(i) => write!(f, "{}", i),  // NEW
            Value::Integer(i) => write!(f, "{}", i),
            // ...
        }
    }
}
```

### 6. Update type validation (`executor.rs`)
Add validation in INSERT to check value fits in SMALLINT range.

---

## Dependencies for Advanced Types

```toml
[dependencies]
# Current
tokio = "1.41"
nom = "7.1"
serde = "1.0"
bincode = "1.3"
thiserror = "2.0"
comfy-table = "7.1"

# For advanced types:
chrono = "0.4"           # Date/Time types
uuid = "1.6"             # UUID type
rust_decimal = "1.33"    # NUMERIC/DECIMAL
serde_json = "1.0"       # Already have for JSON/JSONB
```

---

## Quick Win: Add SMALLINT Right Now?

Would you like me to implement **SMALLINT** as a quick demonstration? It's:
- ✅ Simple (just i16 instead of i64)
- ✅ Useful (space efficiency)
- ✅ ~30 minutes of work
- ✅ Good template for adding other numeric types

Let me know if you want me to add it!
