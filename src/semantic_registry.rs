//! Stable, versioned semantic concept IDs. Names are presentation metadata;
//! the integer identity is the contract and IDs are never recycled.
use crate::types::ConceptId;

pub const VERSION: u16 = 4;
const LOCAL_MASK: u32 = 0x00ff_ffff;

pub const ACTION_NAMESPACE: u32 = 0x0100_0000;
pub const PREDICATE_NAMESPACE: u32 = 0x0200_0000;
pub const PROPERTY_NAMESPACE: u32 = 0x0300_0000;
pub const ENTITY_KIND_NAMESPACE: u32 = 0x0400_0000;
pub const OPERATOR_NAMESPACE: u32 = 0x0500_0000;
pub const VALUE_TYPE_NAMESPACE: u32 = 0x0600_0000;

pub const GIT_STATUS: ConceptId = ConceptId(ACTION_NAMESPACE | 1);
pub const SHOW_DIFF: ConceptId = ConceptId(ACTION_NAMESPACE | 2);
pub const RUN_TESTS: ConceptId = ConceptId(ACTION_NAMESPACE | 3);
pub const LIST_FILES: ConceptId = ConceptId(ACTION_NAMESPACE | 4);
pub const FIND_CHANGED_FILES: ConceptId = ConceptId(ACTION_NAMESPACE | 5);
pub const INSPECT_INTERFACES: ConceptId = ConceptId(ACTION_NAMESPACE | 6);
pub const INSPECT_ROUTES: ConceptId = ConceptId(ACTION_NAMESPACE | 7);
pub const COPY: ConceptId = ConceptId(ACTION_NAMESPACE | 8);
pub const FIND: ConceptId = ConceptId(ACTION_NAMESPACE | 9);
pub const INSPECT_NEIGHBORS: ConceptId = ConceptId(ACTION_NAMESPACE | 10);
pub const DIAGNOSE_NETWORK: ConceptId = ConceptId(ACTION_NAMESPACE | 11);
pub const LIST_SOCKETS: ConceptId = ConceptId(ACTION_NAMESPACE | 12);
pub const LOCATE_PYTHON: ConceptId = ConceptId(ACTION_NAMESPACE | 13);

pub const TARGET: ConceptId = ConceptId(PREDICATE_NAMESPACE | 1);
pub const OBJECT: ConceptId = ConceptId(PREDICATE_NAMESPACE | 2);
pub const SOURCE: ConceptId = ConceptId(PREDICATE_NAMESPACE | 3);
pub const DESTINATION: ConceptId = ConceptId(PREDICATE_NAMESPACE | 4);
pub const TIME: ConceptId = ConceptId(PREDICATE_NAMESPACE | 5);
pub const AFTER: ConceptId = ConceptId(PREDICATE_NAMESPACE | 6);
pub const ON_SUCCESS: ConceptId = ConceptId(PREDICATE_NAMESPACE | 7);
pub const ON_FAILURE: ConceptId = ConceptId(PREDICATE_NAMESPACE | 8);
pub const REQUIRES: ConceptId = ConceptId(PREDICATE_NAMESPACE | 9);
pub const VALUE: ConceptId = ConceptId(PREDICATE_NAMESPACE | 10);

pub const SIZE: ConceptId = ConceptId(PROPERTY_NAMESPACE | 1);

pub const PROJECT: ConceptId = ConceptId(ENTITY_KIND_NAMESPACE | 1);
pub const HOST: ConceptId = ConceptId(ENTITY_KIND_NAMESPACE | 2);
pub const FILE: ConceptId = ConceptId(ENTITY_KIND_NAMESPACE | 3);
pub const DIRECTORY: ConceptId = ConceptId(ENTITY_KIND_NAMESPACE | 4);
pub const FILE_SET: ConceptId = ConceptId(ENTITY_KIND_NAMESPACE | 5);

pub const GREATER_THAN: ConceptId = ConceptId(OPERATOR_NAMESPACE | 1);

pub const BYTES: ConceptId = ConceptId(VALUE_TYPE_NAMESPACE | 1);
pub const YESTERDAY: ConceptId = ConceptId(VALUE_TYPE_NAMESPACE | 2);

pub fn namespace(id: ConceptId) -> u32 {
    id.0 & !LOCAL_MASK
}

pub fn known(id: ConceptId) -> bool {
    name(id).is_some()
}

pub fn name(id: ConceptId) -> Option<&'static str> {
    Some(match id {
        GIT_STATUS => "GIT_STATUS",
        SHOW_DIFF => "SHOW_DIFF",
        RUN_TESTS => "RUN_TESTS",
        LIST_FILES => "LIST_FILES",
        FIND_CHANGED_FILES => "FIND_CHANGED_FILES",
        INSPECT_INTERFACES => "INSPECT_INTERFACES",
        INSPECT_ROUTES => "INSPECT_ROUTES",
        COPY => "COPY",
        FIND => "FIND",
        INSPECT_NEIGHBORS => "INSPECT_NEIGHBORS",
        DIAGNOSE_NETWORK => "DIAGNOSE_NETWORK",
        LIST_SOCKETS => "LIST_SOCKETS",
        LOCATE_PYTHON => "LOCATE_PYTHON",
        TARGET => "TARGET",
        OBJECT => "OBJECT",
        SOURCE => "SOURCE",
        DESTINATION => "DESTINATION",
        TIME => "TIME",
        AFTER => "AFTER",
        ON_SUCCESS => "ON_SUCCESS",
        ON_FAILURE => "ON_FAILURE",
        REQUIRES => "REQUIRES",
        VALUE => "VALUE",
        SIZE => "SIZE",
        PROJECT => "PROJECT",
        HOST => "HOST",
        FILE => "FILE",
        DIRECTORY => "DIRECTORY",
        FILE_SET => "FILE_SET",
        GREATER_THAN => "GREATER_THAN",
        BYTES => "BYTES",
        YESTERDAY => "YESTERDAY",
        _ => return None,
    })
}

pub fn action_to_intent(id: ConceptId) -> Option<crate::types::IntentId> {
    Some(match id {
        GIT_STATUS => crate::types::IntentId(1),
        SHOW_DIFF => crate::types::IntentId(2),
        RUN_TESTS => crate::types::IntentId(3),
        LIST_FILES => crate::types::IntentId(4),
        FIND_CHANGED_FILES => crate::types::IntentId(5),
        INSPECT_INTERFACES => crate::types::IntentId(6),
        INSPECT_ROUTES => crate::types::IntentId(7),
        INSPECT_NEIGHBORS => crate::types::IntentId(8),
        DIAGNOSE_NETWORK => crate::types::IntentId(9),
        LIST_SOCKETS => crate::types::IntentId(10),
        LOCATE_PYTHON => crate::types::IntentId(11),
        COPY | FIND => return None,
        _ => return None,
    })
}

pub fn intent_to_action(id: crate::types::IntentId) -> Option<ConceptId> {
    Some(match id.0 {
        1 => GIT_STATUS,
        2 => SHOW_DIFF,
        3 => RUN_TESTS,
        4 => LIST_FILES,
        5 => FIND_CHANGED_FILES,
        6 => INSPECT_INTERFACES,
        7 => INSPECT_ROUTES,
        8 => INSPECT_NEIGHBORS,
        9 => DIAGNOSE_NETWORK,
        10 => LIST_SOCKETS,
        11 => LOCATE_PYTHON,
        _ => return None,
    })
}

pub fn entity_kind(kind: u16) -> Option<ConceptId> {
    Some(match kind {
        crate::world::PROJECT => PROJECT,
        crate::world::HOST => HOST,
        crate::world::FILE => FILE,
        crate::world::DIRECTORY => DIRECTORY,
        _ => return None,
    })
}
