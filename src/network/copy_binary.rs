use bytes::{BufMut, BytesMut, Buf};
use crate::core::{Value, Column, DataType};
use chrono::{NaiveDate, NaiveDateTime, Duration};
use rust_decimal::Decimal;
use std::io::Cursor;

/// PostgreSQL binary COPY format signature: "PGCOPY\n\377\r\n\0"
pub const COPY_BINARY_SIGNATURE: &[u8; 11] = b"PGCOPY\n\xff\r\n\0";

/// Flags field for COPY binary header (0 = no OIDs)
pub const COPY_BINARY_FLAGS: i32 = 0;

/// PostgreSQL epoch: 2000-01-01 00:00:00 UTC
const PG_EPOCH_DAYS: i64 = 10_957; // Days between 1970-01-01 and 2000-01-01
const PG_EPOCH_MICROSECONDS: i64 = 946_684_800_000_000; // Microseconds between Unix epoch and PG epoch

/// Binary COPY encoder for PostgreSQL format
pub struct BinaryCopyEncoder;

impl BinaryCopyEncoder {
    /// Write COPY binary header (19 bytes)
    /// Format: signature(11) + flags(4) + extension_length(4)
    #[must_use]
    pub fn write_header() -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(19);

        // Signature: PGCOPY\n\377\r\n\0 (11 bytes)
        buf.put_slice(COPY_BINARY_SIGNATURE);

        // Flags: 0 (no OIDs) - network byte order (big-endian)
        buf.put_i32(COPY_BINARY_FLAGS);

        // Extension area length: 0
        buf.put_i32(0);

        buf.to_vec()
    }

    /// Encode a single row to binary format
    /// Format: field_count(2) + [length(4) + data(n)]*
    pub fn encode_row(values: &[Value]) -> Vec<u8> {
        let mut buf = BytesMut::new();

        // Field count (i16, network byte order)
        buf.put_i16(values.len() as i16);

        // Encode each field
        for value in values {
            Self::encode_field(&mut buf, value);
        }

        buf.to_vec()
    }

    /// Encode a single field (length + data)
    /// NULL is represented as length = -1 with no data
    fn encode_field(buf: &mut BytesMut, value: &Value) {
        match value {
            Value::Null => {
                buf.put_i32(-1); // NULL indicator
            }

            // Numeric types
            Value::SmallInt(n) => {
                buf.put_i32(2); // length
                buf.put_i16(*n);
            }
            Value::Integer(n) => {
                buf.put_i32(8); // length
                buf.put_i64(*n);
            }
            Value::Real(f) => {
                buf.put_i32(8); // length
                buf.put_f64(*f);
            }
            Value::Numeric(d) => {
                Self::encode_numeric(buf, d);
            }

            // String types
            Value::Text(s) | Value::Char(s) => {
                let bytes = s.as_bytes();
                buf.put_i32(bytes.len() as i32);
                buf.put_slice(bytes);
            }

            // Boolean
            Value::Boolean(b) => {
                buf.put_i32(1); // length
                buf.put_u8(if *b { 1 } else { 0 });
            }

            // Date/Time types
            Value::Date(d) => {
                Self::encode_date(buf, d);
            }
            Value::Timestamp(ts) => {
                Self::encode_timestamp(buf, ts);
            }
            Value::TimestampTz(ts) => {
                Self::encode_timestamptz(buf, ts);
            }

            // Special types
            Value::Uuid(u) => {
                let bytes = u.as_bytes();
                buf.put_i32(16); // UUID is always 16 bytes
                buf.put_slice(bytes);
            }
            Value::Json(j) => {
                let bytes = j.as_bytes();
                buf.put_i32(bytes.len() as i32);
                buf.put_slice(bytes);
            }
            Value::Bytea(b) => {
                buf.put_i32(b.len() as i32);
                buf.put_slice(b);
            }
            Value::Enum(_, val) => {
                let bytes = val.as_bytes();
                buf.put_i32(bytes.len() as i32);
                buf.put_slice(bytes);
            }
        }
    }

    /// Encode Date as i32 days since 2000-01-01 (PostgreSQL epoch)
    fn encode_date(buf: &mut BytesMut, date: &NaiveDate) {
        let pg_epoch = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
        let days = (*date - pg_epoch).num_days() as i32;
        buf.put_i32(4); // length
        buf.put_i32(days);
    }

    /// Encode Timestamp as i64 microseconds since 2000-01-01 00:00:00
    fn encode_timestamp(buf: &mut BytesMut, ts: &NaiveDateTime) {
        let pg_epoch = NaiveDate::from_ymd_opt(2000, 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .unwrap();
        let micros = (*ts - pg_epoch).num_microseconds().unwrap_or(0);
        buf.put_i32(8); // length
        buf.put_i64(micros);
    }

    /// Encode TimestampTz as i64 microseconds since 2000-01-01 00:00:00 UTC
    fn encode_timestamptz(buf: &mut BytesMut, ts: &chrono::DateTime<chrono::Utc>) {
        let pg_epoch = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let micros = (*ts - pg_epoch).num_microseconds().unwrap_or(0);
        buf.put_i32(8); // length
        buf.put_i64(micros);
    }

    /// Encode Numeric in full PostgreSQL binary format (base-10000)
    /// Format: ndigits(i16) + weight(i16) + sign(i16) + dscale(i16) + digits[i16]
    fn encode_numeric(buf: &mut BytesMut, decimal: &Decimal) {
        // Convert Decimal to PostgreSQL numeric format
        let (digits, weight, sign, dscale) = Self::decimal_to_pg_numeric(decimal);

        // Calculate total length: 4 * i16 (8 bytes) + digits length
        let length = 8 + (digits.len() * 2) as i32;
        buf.put_i32(length);

        // ndigits: number of base-10000 digits
        buf.put_i16(digits.len() as i16);

        // weight: weight of first digit (10000^weight)
        buf.put_i16(weight);

        // sign: 0x0000=positive, 0x4000=negative, 0xC000=NaN
        buf.put_i16(sign);

        // dscale: display scale (decimal places)
        buf.put_i16(dscale);

        // digits: base-10000 digits
        for digit in digits {
            buf.put_i16(digit);
        }
    }

    /// Convert rust_decimal::Decimal to PostgreSQL numeric format
    /// Returns: (digits, weight, sign, dscale)
    fn decimal_to_pg_numeric(decimal: &Decimal) -> (Vec<i16>, i16, i16, i16) {
        // Get mantissa and scale from Decimal
        let mantissa = decimal.mantissa();
        let scale = decimal.scale();

        // Determine sign
        let sign = if mantissa < 0 {
            0x4000 // NUMERIC_NEG
        } else if mantissa == 0 {
            0x0000 // NUMERIC_POS (zero is positive)
        } else {
            0x0000 // NUMERIC_POS
        };

        // Work with absolute value
        let abs_mantissa = mantissa.abs();

        // Convert to base-10000 digits
        let digits = Self::mantissa_to_base10000(abs_mantissa);

        // Calculate weight
        // weight = position of first digit relative to decimal point
        // Example: 123.45 has 3 digits before decimal, weight = 0 (first digit is 10^0)
        // Example: 0.0045 has 0 digits before decimal, weight = -1
        let num_digits_before_decimal = if abs_mantissa == 0 {
            0
        } else {
            abs_mantissa.to_string().len() as i32 - scale as i32
        };

        // In base-10000, each digit represents 4 decimal digits
        // weight = (num_digits_before_decimal - 1) / 4
        let weight = ((num_digits_before_decimal + 3) / 4 - 1) as i16;

        (digits, weight, sign, scale as i16)
    }

    /// Convert i128 mantissa to base-10000 digits (most significant first)
    fn mantissa_to_base10000(mut mantissa: i128) -> Vec<i16> {
        if mantissa == 0 {
            return vec![0];
        }

        let mut digits = Vec::new();
        while mantissa > 0 {
            digits.push((mantissa % 10000) as i16);
            mantissa /= 10000;
        }

        // Reverse to get most significant digit first
        digits.reverse();
        digits
    }

    /// Write COPY binary trailer (2 bytes: i16 value -1)
    #[must_use]
    pub fn write_trailer() -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(2);
        buf.put_i16(-1); // EOF marker
        buf.to_vec()
    }
}

/// Binary COPY decoder for PostgreSQL format
pub struct BinaryCopyDecoder;

impl BinaryCopyDecoder {
    /// Read and validate binary header
    /// Returns Ok(()) if header is valid, Err with message otherwise
    pub fn read_header(cursor: &mut Cursor<&[u8]>) -> Result<(), String> {
        // Read signature (11 bytes)
        if cursor.remaining() < 11 {
            return Err("Insufficient data for signature".to_string());
        }

        let mut sig = [0u8; 11];
        cursor.copy_to_slice(&mut sig);

        if &sig != COPY_BINARY_SIGNATURE {
            return Err("Invalid COPY binary signature".to_string());
        }

        // Read flags (4 bytes)
        if cursor.remaining() < 4 {
            return Err("Insufficient data for flags".to_string());
        }
        let flags = cursor.get_i32();

        // Check for OID flag (bit 16)
        if flags & 0x10000 != 0 {
            return Err("OID columns not supported".to_string());
        }

        // Read extension area length (4 bytes)
        if cursor.remaining() < 4 {
            return Err("Insufficient data for extension length".to_string());
        }
        let ext_len = cursor.get_i32();

        // Skip extension area if present
        if ext_len > 0 {
            if cursor.remaining() < ext_len as usize {
                return Err("Insufficient data for extension area".to_string());
            }
            cursor.advance(ext_len as usize);
        }

        Ok(())
    }

    /// Decode a single row
    /// Returns Ok(Some(values)) for a row, Ok(None) for EOF marker, Err for errors
    pub fn decode_row(cursor: &mut Cursor<&[u8]>, columns: &[Column]) -> Result<Option<Vec<Value>>, String> {
        // Read field count (2 bytes)
        if cursor.remaining() < 2 {
            return Err("Insufficient data for field count".to_string());
        }
        let field_count = cursor.get_i16();

        // Check for EOF marker
        if field_count == -1 {
            return Ok(None);
        }

        if field_count < 0 {
            return Err(format!("Invalid field count: {field_count}"));
        }

        if field_count as usize != columns.len() {
            return Err(format!(
                "Field count mismatch: expected {}, got {field_count}",
                columns.len()
            ));
        }

        // Decode each field
        let mut values = Vec::with_capacity(field_count as usize);
        for col in columns {
            let value = Self::decode_field(cursor, &col.data_type)?;
            values.push(value);
        }

        Ok(Some(values))
    }

    /// Decode a single field
    fn decode_field(cursor: &mut Cursor<&[u8]>, data_type: &DataType) -> Result<Value, String> {
        // Read field length (4 bytes)
        if cursor.remaining() < 4 {
            return Err("Insufficient data for field length".to_string());
        }
        let len = cursor.get_i32();

        // NULL check
        if len == -1 {
            return Ok(Value::Null);
        }

        if len < 0 {
            return Err(format!("Invalid field length: {len}"));
        }

        // Read field data
        if cursor.remaining() < len as usize {
            return Err(format!("Insufficient data for field: expected {len} bytes"));
        }

        let start_pos = cursor.position() as usize;
        let data = &cursor.get_ref()[start_pos..start_pos + len as usize];
        cursor.advance(len as usize);

        // Decode based on type
        Self::parse_field_data(data, data_type)
    }

    /// Parse field data based on type
    fn parse_field_data(data: &[u8], data_type: &DataType) -> Result<Value, String> {
        match data_type {
            DataType::SmallInt => {
                if data.len() != 2 {
                    return Err(format!("Invalid SmallInt length: {}", data.len()));
                }
                let val = i16::from_be_bytes([data[0], data[1]]);
                Ok(Value::SmallInt(val))
            }

            DataType::Integer | DataType::Serial | DataType::BigSerial => {
                if data.len() != 8 {
                    return Err(format!("Invalid Integer length: {}", data.len()));
                }
                let bytes: [u8; 8] = data.try_into().unwrap();
                let val = i64::from_be_bytes(bytes);
                Ok(Value::Integer(val))
            }

            DataType::Real => {
                if data.len() != 8 {
                    return Err(format!("Invalid Real length: {}", data.len()));
                }
                let bytes: [u8; 8] = data.try_into().unwrap();
                let val = f64::from_be_bytes(bytes);
                Ok(Value::Real(val))
            }

            DataType::Numeric { .. } => {
                Self::decode_numeric(data)
            }

            DataType::Boolean => {
                if data.len() != 1 {
                    return Err(format!("Invalid Boolean length: {}", data.len()));
                }
                Ok(Value::Boolean(data[0] != 0))
            }

            DataType::Text | DataType::Varchar { .. } | DataType::Char { .. } => {
                let s = String::from_utf8(data.to_vec())
                    .map_err(|e| format!("Invalid UTF-8: {e}"))?;
                Ok(match data_type {
                    DataType::Char { .. } => Value::Char(s),
                    _ => Value::Text(s),
                })
            }

            DataType::Date => {
                if data.len() != 4 {
                    return Err(format!("Invalid Date length: {}", data.len()));
                }
                let bytes: [u8; 4] = data.try_into().unwrap();
                let days = i32::from_be_bytes(bytes);
                let pg_epoch = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
                let date = pg_epoch + Duration::days(days as i64);
                Ok(Value::Date(date))
            }

            DataType::Timestamp => {
                if data.len() != 8 {
                    return Err(format!("Invalid Timestamp length: {}", data.len()));
                }
                let bytes: [u8; 8] = data.try_into().unwrap();
                let micros = i64::from_be_bytes(bytes);
                let pg_epoch = NaiveDate::from_ymd_opt(2000, 1, 1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .unwrap();
                let ts = pg_epoch + Duration::microseconds(micros);
                Ok(Value::Timestamp(ts))
            }

            DataType::TimestampTz => {
                if data.len() != 8 {
                    return Err(format!("Invalid TimestampTz length: {}", data.len()));
                }
                let bytes: [u8; 8] = data.try_into().unwrap();
                let micros = i64::from_be_bytes(bytes);
                let pg_epoch = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc);
                let ts = pg_epoch + Duration::microseconds(micros);
                Ok(Value::TimestampTz(ts))
            }

            DataType::Uuid => {
                if data.len() != 16 {
                    return Err(format!("Invalid UUID length: {}", data.len()));
                }
                let u = uuid::Uuid::from_slice(data)
                    .map_err(|e| format!("Invalid UUID: {e}"))?;
                Ok(Value::Uuid(u))
            }

            DataType::Bytea => {
                Ok(Value::Bytea(data.to_vec()))
            }

            DataType::Json | DataType::Jsonb => {
                let s = String::from_utf8(data.to_vec())
                    .map_err(|e| format!("Invalid JSON UTF-8: {e}"))?;
                Ok(Value::Json(s))
            }

            DataType::Enum { name, .. } => {
                let s = String::from_utf8(data.to_vec())
                    .map_err(|e| format!("Invalid Enum UTF-8: {e}"))?;
                Ok(Value::Enum(name.clone(), s))
            }
        }
    }

    /// Decode PostgreSQL numeric binary format to Decimal
    fn decode_numeric(data: &[u8]) -> Result<Value, String> {
        if data.len() < 8 {
            return Err(format!("Invalid Numeric length: {} (minimum 8)", data.len()));
        }

        let mut cursor = Cursor::new(data);

        // Read header fields
        let ndigits = cursor.get_i16();
        let weight = cursor.get_i16();
        let sign = cursor.get_i16();
        let dscale = cursor.get_i16();

        if ndigits < 0 {
            return Err(format!("Invalid ndigits: {ndigits}"));
        }

        // Check for NaN
        if sign == 0xC000_u16 as i16 {
            return Err("NaN not supported in Decimal".to_string());
        }

        // Read base-10000 digits
        let expected_len = 8 + (ndigits as usize * 2);
        if data.len() != expected_len {
            return Err(format!(
                "Invalid Numeric data length: expected {expected_len}, got {}",
                data.len()
            ));
        }

        let mut digits = Vec::with_capacity(ndigits as usize);
        for _ in 0..ndigits {
            let digit = cursor.get_i16();
            if digit < 0 || digit >= 10000 {
                return Err(format!("Invalid base-10000 digit: {digit}"));
            }
            digits.push(digit);
        }

        // Convert to Decimal
        let decimal = Self::pg_numeric_to_decimal(&digits, weight, sign, dscale)?;
        Ok(Value::Numeric(decimal))
    }

    /// Convert PostgreSQL numeric to rust_decimal::Decimal
    fn pg_numeric_to_decimal(digits: &[i16], _weight: i16, sign: i16, dscale: i16) -> Result<Decimal, String> {
        if digits.is_empty() {
            return Ok(Decimal::ZERO);
        }

        // Convert base-10000 digits to mantissa
        let mut mantissa: i128 = 0;
        for &digit in digits {
            mantissa = mantissa * 10000 + digit as i128;
        }

        // Apply sign
        if sign == 0x4000 {
            mantissa = -mantissa;
        }

        // Calculate actual scale
        // weight tells us the position of the first digit
        // dscale tells us the number of decimal places
        let scale = dscale as u32;

        // Create Decimal from mantissa and scale
        Ok(Decimal::from_i128_with_scale(mantissa, scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_header() {
        let header = BinaryCopyEncoder::write_header();
        assert_eq!(header.len(), 19);
        assert_eq!(&header[0..11], COPY_BINARY_SIGNATURE);

        // Verify flags (bytes 11-14) are 0
        let flags = i32::from_be_bytes([header[11], header[12], header[13], header[14]]);
        assert_eq!(flags, 0);

        // Verify extension length (bytes 15-18) are 0
        let ext_len = i32::from_be_bytes([header[15], header[16], header[17], header[18]]);
        assert_eq!(ext_len, 0);
    }

    #[test]
    fn test_binary_trailer() {
        let trailer = BinaryCopyEncoder::write_trailer();
        assert_eq!(trailer.len(), 2);
        let eof = i16::from_be_bytes([trailer[0], trailer[1]]);
        assert_eq!(eof, -1);
    }

    #[test]
    fn test_encode_null() {
        let values = vec![Value::Null];
        let row = BinaryCopyEncoder::encode_row(&values);

        // Field count (2) + NULL length (-1 = 4 bytes) = 6 bytes
        assert_eq!(row.len(), 6);

        // Check field count
        let field_count = i16::from_be_bytes([row[0], row[1]]);
        assert_eq!(field_count, 1);

        // Check NULL indicator
        let null_len = i32::from_be_bytes([row[2], row[3], row[4], row[5]]);
        assert_eq!(null_len, -1);
    }

    #[test]
    fn test_encode_integers() {
        let values = vec![
            Value::SmallInt(42),
            Value::Integer(1234567890),
        ];
        let row = BinaryCopyEncoder::encode_row(&values);

        // Field count (2) + SmallInt (4+2) + Integer (4+8) = 20 bytes
        assert_eq!(row.len(), 20);

        // Verify SmallInt value
        let smallint = i16::from_be_bytes([row[6], row[7]]);
        assert_eq!(smallint, 42);

        // Verify Integer value
        let integer = i64::from_be_bytes([
            row[12], row[13], row[14], row[15],
            row[16], row[17], row[18], row[19],
        ]);
        assert_eq!(integer, 1234567890);
    }

    #[test]
    fn test_encode_boolean() {
        let values = vec![Value::Boolean(true), Value::Boolean(false)];
        let row = BinaryCopyEncoder::encode_row(&values);

        // Field count (2) + 2*(length(4) + data(1)) = 12 bytes
        assert_eq!(row.len(), 12);

        // Verify true value
        assert_eq!(row[6], 1);

        // Verify false value
        assert_eq!(row[11], 0);
    }

    #[test]
    fn test_encode_text() {
        let values = vec![Value::Text("hello".to_string())];
        let row = BinaryCopyEncoder::encode_row(&values);

        // Field count (2) + length (4) + data (5) = 11 bytes
        assert_eq!(row.len(), 11);

        // Verify text value
        assert_eq!(&row[6..11], b"hello");
    }
}
