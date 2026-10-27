use super::*;
use crate::test_support::tool;

#[test]
fn catalog_rejects_duplicate_names_and_finds_the_advertised_definition() {
    let first = tool("browse");
    let second = tool("alpha");
    assert_eq!(
        ToolCatalog::new(vec![first.clone(), second.clone(), first.clone()]).unwrap_err(),
        ToolCatalogError::DuplicateName("browse".into()),
    );
    let catalog = ToolCatalog::new(vec![first.clone(), second.clone()]).unwrap();
    assert_eq!(catalog.definitions(), &[first.clone(), second]);
    assert_eq!(catalog.find("browse"), Some(&first));
    assert!(catalog.find("missing").is_none());
}
