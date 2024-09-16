use lazy_static::lazy_static;
use pflow_metamodel::*;

/// Expected CID needs to be updated if the model definition changes
fn check_for_update(cid: &str, model: &Model) {
    let zblob = model.net.to_zblob();
    assert_eq!(cid, zblob.ipfs_cid, "[MODIFIED] {}\n[pflow] https://pflow.dev?z={:}\n",zblob.ipfs_cid, zblob.base64_zipped);
}

#[macro_export]
macro_rules! pflow_model {
    ($name:ident, $oid_name:ident, $expected_cid:expr, $($dsl:tt)*) => {
        lazy_static! {
            pub static ref $name: Model = {
                let model = pflow_dsl! {
                    $($dsl)*
                };
                check_for_update($expected_cid, &model);
                model
            };

            pub static ref $oid_name: String = $expected_cid.to_string();
        }
    };
}

/// Declare the model and its OID outside of the test module
/// Expected CID needs to be updated if the model changes
pflow_model!(TEST_MODEL,
    TEST_CID, "zb2rhnJobUyvbfHB3yrFDZ3Z4KDhJRSNtrpeU272zri2LYfDT",
    declare "petriNet"
    cell "place0", 0, 3, [100, 180]
    func "txn0", "default", [20, 100]
    func "txn1", "default", [180, 100]
    func "txn2", "default", [20, 260]
    func "txn3", "default", [180, 260]
    arrow "txn0", "place0", 1
    arrow "place0", "txn1", 3
    guard "txn2", "place0", 3
    guard "place0", "txn3", 1
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pflow_dsl() {
        check_for_update(&**TEST_CID, &TEST_MODEL);
    }

}