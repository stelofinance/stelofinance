//! Display labels for module enums.

use crate::module_bindings::{LedgerKind, Role};

pub fn role_label(role: Role) -> &'static str {
	match role {
		Role::Read => "Read",
		Role::Write => "Write",
		Role::Admin => "Admin",
		Role::Owner => "Owner",
	}
}

pub fn ledger_kind_label(kind: LedgerKind) -> &'static str {
	match kind {
		LedgerKind::Physical => "In-game item",
		LedgerKind::Digital => "Digital",
		LedgerKind::Derivation => "Derivation",
	}
}
