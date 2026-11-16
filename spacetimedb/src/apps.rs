use crate::require_registered_user;
use crate::tables::*;
use spacetimedb::{
	Identity, ProcedureContext, ReducerContext, ScheduleAt, Table, procedure, reducer,
};
use std::time::Duration;

const TICKET_TTL: Duration = Duration::from_secs(15 * 60);
/// App names are ASCII letters, digits and the separators `-` `_` `.` (RFC 3986
/// unreserved, minus `~`), `MIN_APP_NAME_LEN..=MAX_APP_NAME_LEN` long. They start
/// and end with a letter or digit and never have two separators in a row.
/// Keep in sync with `src/app/app/me/apps.rs` (`valid_app_name`).
const MIN_APP_NAME_LEN: usize = 3;
const MAX_APP_NAME_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Ticket reducers (human user)
// ---------------------------------------------------------------------------

/// Create (or replace own) a Create-purpose ticket for a new app name + the app's identity (hex).
#[reducer]
pub fn create_app_ticket(ctx: &ReducerContext, name: String, sub: String) -> Result<(), String> {
	require_registered_user(ctx)?;
	let name = normalize_name(name)?;
	let sub = normalize_sub(sub)?;

	if ctx.db.app().name().find(&name).is_some() {
		return Err("app name already taken".to_string());
	}

	upsert_ticket(ctx, AppTicketPurpose::Create, name, sub)
}

/// Create (or replace own) a Replace-purpose ticket for an existing app you own.
#[reducer]
pub fn replace_app_ticket(ctx: &ReducerContext, name: String, sub: String) -> Result<(), String> {
	require_registered_user(ctx)?;
	let name = name.trim().to_string();
	let sub = normalize_sub(sub)?;

	let app = ctx
		.db
		.app()
		.name()
		.find(&name)
		.ok_or_else(|| "app not found".to_string())?;
	if app.created_by != ctx.sender() {
		return Err("only the app owner can replace its identity".to_string());
	}

	upsert_ticket(ctx, AppTicketPurpose::Replace, name, sub)
}

/// Delete an app you own. Its account grants and any open ticket for its name go
/// with it; the app's identity can no longer connect (no app row, no ticket).
#[reducer]
pub fn delete_app(ctx: &ReducerContext, id: Identity) -> Result<(), String> {
	require_registered_user(ctx)?;
	let app = ctx
		.db
		.app()
		.id()
		.find(&id)
		.ok_or_else(|| "app not found".to_string())?;
	if app.created_by != ctx.sender() {
		return Err("only the app owner can delete it".to_string());
	}

	let grants: Vec<u64> = ctx
		.db
		.account_member()
		.member_id()
		.filter(&id)
		.map(|m| m.id)
		.collect();
	for grant in &grants {
		ctx.db.account_member().id().delete(grant);
	}

	// A pending Replace ticket would otherwise fail on connect ("app not found").
	if let Some(ticket) = ctx.db.app_ticket().name().find(&app.name) {
		ctx.db.app_ticket().id().delete(&ticket.id);
	}

	ctx.db.app().id().delete(&id);

	log::info!(
		"app deleted name={} id={} by={} grants_removed={}",
		app.name,
		id,
		app.created_by,
		grants.len()
	);
	Ok(())
}

/// Scheduled cleanup when a ticket's `expires_at` fires (host deletes the row first).
#[procedure]
pub fn expire_app_ticket(ctx: &mut ProcedureContext, ticket: AppTicket) {
	if ctx.sender() != ctx.database_identity() {
		log::warn!(
			"expire_app_ticket rejected: non-scheduler caller {}",
			ctx.sender()
		);
		return;
	}
	log::info!(
		"app_ticket expired name={} sub={} purpose={:?}",
		ticket.name,
		ticket.sub,
		ticket.purpose
	);
}

// ---------------------------------------------------------------------------
// Connect-time bind (called from client_connected)
// ---------------------------------------------------------------------------

/// If a locally-signed connection has a matching open ticket (keyed by identity hex), create or
/// replace the app.
/// Returns `Ok(true)` if an app now exists for `sender` (created, replaced, or already handled).
pub(crate) fn try_fulfill_app_ticket(
	ctx: &ReducerContext,
	sender: Identity,
	sub: &str,
) -> Result<(), String> {
	let sub = sub.to_string();
	let ticket = ctx
		.db
		.app_ticket()
		.sub()
		.find(&sub)
		.ok_or_else(|| "no app registration ticket for this identity".to_string())?;

	match ticket.purpose {
		AppTicketPurpose::Create => fulfill_create(ctx, sender, &ticket)?,
		AppTicketPurpose::Replace => fulfill_replace(ctx, sender, &ticket)?,
	}

	ctx.db.app_ticket().id().delete(&ticket.id);
	Ok(())
}

fn fulfill_create(
	ctx: &ReducerContext,
	sender: Identity,
	ticket: &AppTicket,
) -> Result<(), String> {
	if ctx.db.user().id().find(&sender).is_some() {
		return Err("identity cannot be both user and app".to_string());
	}
	if ctx.db.app().id().find(&sender).is_some() {
		return Err("identity already registered as an app".to_string());
	}
	if ctx.db.app().name().find(&ticket.name).is_some() {
		return Err("app name already taken".to_string());
	}

	ctx.db.app().insert(App {
		id: sender,
		name: ticket.name.clone(),
		name_normalized: ticket.name.to_ascii_uppercase(),
		created_by: ticket.created_by,
		updated_at: ctx.timestamp,
		created_at: ctx.timestamp,
	});

	log::info!(
		"app created name={} id={} by={}",
		ticket.name,
		sender,
		ticket.created_by
	);
	Ok(())
}

fn fulfill_replace(
	ctx: &ReducerContext,
	sender: Identity,
	ticket: &AppTicket,
) -> Result<(), String> {
	if ctx.db.user().id().find(&sender).is_some() {
		return Err("identity cannot be both user and app".to_string());
	}
	if ctx.db.app().id().find(&sender).is_some() {
		return Err("new identity already registered as an app".to_string());
	}

	let old_app = ctx
		.db
		.app()
		.name()
		.find(&ticket.name)
		.ok_or_else(|| "app not found for replace".to_string())?;

	if old_app.created_by != ticket.created_by {
		return Err("ticket owner mismatch".to_string());
	}

	let old_id = old_app.id;
	if old_id == sender {
		// Same identity reconnect — nothing to migrate.
		log::info!(
			"replace_app_identity no-op name={} id={}",
			ticket.name,
			sender
		);
		return Ok(());
	}

	// Snapshot grants, then swap app identity on memberships.
	let grants: Vec<AccountMember> = ctx
		.db
		.account_member()
		.member_id()
		.filter(&old_id)
		.collect();

	for g in &grants {
		ctx.db.account_member().id().delete(&g.id);
	}

	ctx.db.app().id().delete(&old_id);

	ctx.db.app().insert(App {
		id: sender,
		name: old_app.name.clone(),
		name_normalized: old_app.name.to_ascii_uppercase(),
		created_by: old_app.created_by,
		updated_at: ctx.timestamp,
		created_at: old_app.created_at,
	});

	for g in grants {
		ctx.db.account_member().insert(AccountMember {
			id: 0,
			account_id: g.account_id,
			member_id: sender,
			kind: MemberKind::App,
			role: g.role,
			updated_at: ctx.timestamp,
			created_at: g.created_at,
		});
	}

	log::info!(
		"app identity replaced name={} old={} new={} by={}",
		ticket.name,
		old_id,
		sender,
		ticket.created_by
	);
	Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn upsert_ticket(
	ctx: &ReducerContext,
	purpose: AppTicketPurpose,
	name: String,
	sub: String,
) -> Result<(), String> {
	let sender = ctx.sender();

	// Name conflict on another user's ticket.
	if let Some(existing) = ctx.db.app_ticket().name().find(&name.to_string()) {
		if existing.created_by != sender {
			return Err("app name already taken".to_string());
		}
		// Same user: drop ticket so we can re-insert (unique name + sub).
		ctx.db.app_ticket().id().delete(&existing.id);
	}

	// Sub conflict on another user's ticket.
	if let Some(existing) = ctx.db.app_ticket().sub().find(&sub) {
		if existing.created_by != sender {
			return Err("this identity is already reserved by another ticket".to_string());
		}
		// Same user may have had a ticket under a different name.
		ctx.db.app_ticket().id().delete(&existing.id);
	}

	let expires = ctx.timestamp + TICKET_TTL;
	ctx.db.app_ticket().insert(AppTicket {
		id: 0,
		expires_at: ScheduleAt::Time(expires),
		created_by: sender,
		name: name.clone(),
		sub: sub.clone(),
		purpose,
		created_at: ctx.timestamp,
	});

	log::info!(
		"app_ticket purpose={:?} name={} sub={} by={} expires_in=15m",
		purpose,
		name,
		sub,
		sender
	);
	Ok(())
}

fn normalize_name(name: String) -> Result<String, String> {
	let name = name.trim().to_string();
	if name.is_empty() {
		return Err("app name required".to_string());
	}
	if name.len() < MIN_APP_NAME_LEN || name.len() > MAX_APP_NAME_LEN {
		return Err(format!(
			"app name must be {MIN_APP_NAME_LEN}-{MAX_APP_NAME_LEN} characters"
		));
	}
	let b = name.as_bytes();
	if !b.iter().all(|&c| c.is_ascii_alphanumeric() || is_name_sep(c)) {
		return Err("app name may only contain letters, numbers, '-', '_' and '.'".to_string());
	}
	if !b[0].is_ascii_alphanumeric() || !b[b.len() - 1].is_ascii_alphanumeric() {
		return Err("app name must start and end with a letter or number".to_string());
	}
	if b.windows(2).any(|w| is_name_sep(w[0]) && is_name_sep(w[1])) {
		return Err("app name cannot have two symbols in a row".to_string());
	}
	Ok(name)
}

fn is_name_sep(c: u8) -> bool {
	matches!(c, b'-' | b'_' | b'.')
}

fn normalize_sub(sub: String) -> Result<String, String> {
	let sub = sub.trim().to_ascii_lowercase();
	if sub.len() != 64 || !sub.bytes().all(|b| b.is_ascii_hexdigit()) {
		return Err("sub must be an identity hex string".to_string());
	}
	Ok(sub)
}
