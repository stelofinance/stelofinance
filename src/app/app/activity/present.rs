//! Filter-relative verbs and card copy for Activity.

use crate::module_bindings::{MyAccountRow, MyTransferRow, TransferKind, TransferState};
use crate::stdb::{display_amount, format_qty, sender_receiver, timestamp_micros};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
	Received,
	Sent,
	Moved,
	Issued,
	Redeemed,
}

impl Verb {
	pub fn label(self) -> &'static str {
		match self {
			Self::Received => "Received",
			Self::Sent => "Sent",
			Self::Moved => "Moved",
			Self::Issued => "Issued",
			Self::Redeemed => "Redeemed",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountSign {
	In,
	Out,
	Flat,
}

impl AmountSign {
	fn for_verb(verb: Verb) -> Self {
		match verb {
			Verb::Received => Self::In,
			Verb::Sent | Verb::Issued | Verb::Redeemed => Self::Out,
			Verb::Moved => Self::Flat,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
	pub verb: Verb,
	pub title: String,
	pub subtitle: String,
	pub sign: AmountSign,
}

#[derive(Clone, Debug)]
pub struct TransferCard {
	pub id: u64,
	pub created_micros: i64,
	pub ledger_name: String,
	pub qty: String,
	pub memo: Option<String>,
	pub state_label: Option<&'static str>,
	pub involved: Vec<u64>,
	pub all: Line,
	pub per_account: Vec<(u64, Line)>,
}

pub fn transfer_card(tr: &MyTransferRow, accounts: &[MyAccountRow]) -> Option<TransferCard> {
	let (sender, receiver) = sender_receiver(tr.kind, tr.credit_account_id, tr.debit_account_id);
	let own_sender = accounts.iter().any(|a| a.account_id == sender);
	let own_receiver = accounts.iter().any(|a| a.account_id == receiver);
	if !own_sender && !own_receiver {
		return None;
	}

	let sender_name = party_name(sender, tr, accounts);
	let receiver_name = party_name(receiver, tr, accounts);

	let mut involved = Vec::new();
	if own_sender {
		involved.push(sender);
	}
	if own_receiver && receiver != sender {
		involved.push(receiver);
	}

	let all = line(
		tr.kind,
		own_sender,
		own_receiver,
		None,
		&sender_name,
		&receiver_name,
	);
	let mut per_account = Vec::new();
	if own_sender {
		per_account.push((
			sender,
			line(
				tr.kind,
				own_sender,
				own_receiver,
				Some(true),
				&sender_name,
				&receiver_name,
			),
		));
	}
	if own_receiver && receiver != sender {
		per_account.push((
			receiver,
			line(
				tr.kind,
				own_sender,
				own_receiver,
				Some(false),
				&sender_name,
				&receiver_name,
			),
		));
	}

	Some(TransferCard {
		id: tr.id,
		created_micros: timestamp_micros(&tr.created_at),
		ledger_name: tr.ledger_name.clone(),
		qty: format_qty(display_amount(tr), tr.ledger_scale),
		memo: tr
			.memo
			.as_deref()
			.map(str::trim)
			.filter(|s| !s.is_empty())
			.map(str::to_owned),
		state_label: state_label(tr.state),
		involved,
		all,
		per_account,
	})
}

fn state_label(state: TransferState) -> Option<&'static str> {
	match state {
		TransferState::Posted => None,
		TransferState::Pending => Some("Pending"),
		TransferState::PostPending | TransferState::VoidPending => Some("Finalizing"),
	}
}

/// `filter_as_sender`: `None` = All accounts; `Some(true)` = looking as sender.
fn line(
	kind: TransferKind,
	own_sender: bool,
	own_receiver: bool,
	filter_as_sender: Option<bool>,
	sender_name: &str,
	receiver_name: &str,
) -> Line {
	let verb = verb_for(kind, own_sender, own_receiver, filter_as_sender);
	let sign = AmountSign::for_verb(verb);
	let (title, subtitle) = match (filter_as_sender, verb) {
		(None, Verb::Moved) => (
			format!("{sender_name} → {receiver_name}"),
			verb.label().into(),
		),
		(None, _) if own_sender && !own_receiver => (
			receiver_name.to_owned(),
			format!("{} from {sender_name}", verb.label()),
		),
		(None, _) => (
			sender_name.to_owned(),
			format!("{} into {receiver_name}", verb.label()),
		),
		(Some(true), _) => (receiver_name.to_owned(), verb.label().into()),
		(Some(false), _) => (sender_name.to_owned(), verb.label().into()),
	};
	Line {
		verb,
		title,
		subtitle,
		sign,
	}
}

pub fn verb_for(
	kind: TransferKind,
	own_sender: bool,
	own_receiver: bool,
	filter_as_sender: Option<bool>,
) -> Verb {
	if filter_as_sender.is_none() && own_sender && own_receiver {
		return Verb::Moved;
	}
	let as_sender = match filter_as_sender {
		None => own_sender && !own_receiver,
		Some(flag) => flag,
	};
	if as_sender {
		match kind {
			TransferKind::Issue => Verb::Issued,
			TransferKind::Redeem => Verb::Redeemed,
			TransferKind::Asset | TransferKind::Liability => Verb::Sent,
		}
	} else {
		Verb::Received
	}
}

fn party_name(account_id: u64, tr: &MyTransferRow, accounts: &[MyAccountRow]) -> String {
	if let Some(acc) = accounts.iter().find(|a| a.account_id == account_id) {
		return account_title(acc);
	}
	let (address, username) = if account_id == tr.credit_account_id {
		(tr.credit_address.as_str(), tr.credit_username.as_deref())
	} else {
		(tr.debit_address.as_str(), tr.debit_username.as_deref())
	};
	if let Some(user) = username.map(str::trim).filter(|s| !s.is_empty()) {
		format!("@{user}")
	} else {
		format!("#{address}")
	}
}

pub fn account_title(acc: &MyAccountRow) -> String {
	acc.label
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(|| format!("#{}", acc.address))
}

#[cfg(test)]
mod tests {
	use super::{AmountSign, Verb, verb_for};
	use crate::module_bindings::TransferKind;

	#[test]
	fn all_internal_is_moved() {
		assert_eq!(verb_for(TransferKind::Asset, true, true, None), Verb::Moved);
	}

	#[test]
	fn filter_internal_is_directional() {
		assert_eq!(
			verb_for(TransferKind::Asset, true, true, Some(true)),
			Verb::Sent
		);
		assert_eq!(
			verb_for(TransferKind::Asset, true, true, Some(false)),
			Verb::Received
		);
	}

	#[test]
	fn issuer_verbs() {
		assert_eq!(
			verb_for(TransferKind::Issue, true, false, None),
			Verb::Issued
		);
		assert_eq!(
			verb_for(TransferKind::Issue, false, true, None),
			Verb::Received
		);
		assert_eq!(
			verb_for(TransferKind::Redeem, true, false, None),
			Verb::Redeemed
		);
		assert_eq!(
			verb_for(TransferKind::Redeem, false, true, None),
			Verb::Received
		);
	}

	#[test]
	fn everyday_asset() {
		assert_eq!(verb_for(TransferKind::Asset, true, false, None), Verb::Sent);
		assert_eq!(
			verb_for(TransferKind::Asset, false, true, None),
			Verb::Received
		);
	}

	#[test]
	fn signs() {
		assert_eq!(AmountSign::for_verb(Verb::Received), AmountSign::In);
		assert_eq!(AmountSign::for_verb(Verb::Issued), AmountSign::Out);
		assert_eq!(AmountSign::for_verb(Verb::Moved), AmountSign::Flat);
	}
}
