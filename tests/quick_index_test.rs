// Quick test to verify git works before committing large changes
use postgrustql::index::BTreeIndex;
use postgrustql::types::Value;

#[test]
fn test_btree_basic() {
    let mut idx = BTreeIndex::new("test_idx".to_string(), "test_tbl".to_string(), "id".to_string(), false);
    idx.insert(&Value::Integer(42), 0).unwrap();
    assert_eq!(idx.search(&Value::Integer(42)), vec![0]);
}
