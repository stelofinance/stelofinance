// Command seed-hexcoin bootstraps a local/dev "hexcoin" ledger for STDB testing.
//
// It connects as a BitAuth admin (JWT), then:
//  1. create_ledger "hexcoin" scale=3 Physical
//  2. create_account credit address=STELOBANK
//  3. create_account ×10 debit (auto address)
//  4. create_transfer issue 100_000 from STELOBANK → each debit (posted)
//
// Private account rows are not visible to BitAuth clients. Account IDs are
// resolved via `spacetime sql` which must run as the **database owner**
// (the identity that published the module). Reducers still use the JWT.
//
// Usage:
//
//	export STDB_TOKEN='eyJ...'   # BitAuth ID token for an is_admin user
//	go run ./scripts/seed-hexcoin
//
//	# optional:
//	go run ./scripts/seed-hexcoin -token "$STDB_TOKEN" -host http://127.0.0.1:3000 -database stelofinance
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"os"
	"os/exec"
	"strings"
	"time"

	"go.digitalxero.dev/spacetimedb-client/bsatn"
	"go.digitalxero.dev/spacetimedb-client/client"
	"go.digitalxero.dev/spacetimedb-client/types"

	"github.com/stelofinance/stelofinance/internal/stdb"
)

const (
	ledgerName    = "hexcoin"
	ledgerScale   = uint8(3)
	creditAddress = "STELOBANK"
	debitAccountN = 10
	issueAmount   = uint64(100_000) // scale 3 → 100.000 hexcoin per wallet
)

// LedgerKind / AccountKind tags match Rust enum declaration order in tables.rs.
const (
	ledgerKindDigital    = 0
	ledgerKindDerivation = 1
	ledgerKindPhysical   = 2

	accountKindCredit = 0
	accountKindDebit  = 1
)

func main() {
	log.SetFlags(0)

	token := flag.String("token", envOr("STDB_TOKEN", ""), "BitAuth ID token for an admin user (or STDB_TOKEN)")
	host := flag.String("host", envOr("STDB_HOST", "http://127.0.0.1:3000"), "STDB HTTP host")
	database := flag.String("database", envOr("STDB_DATABASE", "stelofinance"), "database name")
	server := flag.String("server", "local", "spacetime CLI server nickname for owner SQL lookups")
	flag.Parse()

	if strings.TrimSpace(*token) == "" {
		log.Fatal("missing -token / STDB_TOKEN (BitAuth ID token for admin user)")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()

	reducerErrs := make(chan error, 16)
	connected := make(chan client.DbConnection, 1)

	wsURI := stdb.WebsocketURI(*host)
	conn, err := client.NewDbConnection().
		WithUri(wsURI).
		WithDatabaseName(*database).
		WithToken(*token).
		OnConnect(func(c client.DbConnection, id types.Identity, _ string) {
			log.Printf("connected identity=%s", id)
			select {
			case connected <- c:
			default:
			}
		}).
		OnConnectError(func(err error) {
			log.Printf("connect error: %v", err)
		}).
		OnReducerError(func(err error) {
			select {
			case reducerErrs <- err:
			default:
			}
		}).
		Build(ctx)
	if err != nil {
		log.Fatalf("build connection: %v", err)
	}

	runDone := make(chan error, 1)
	go func() { runDone <- conn.Run(ctx) }()

	var c client.DbConnection
	select {
	case <-ctx.Done():
		log.Fatal("timed out waiting for connect")
	case err := <-runDone:
		log.Fatalf("run ended before connect: %v", err)
	case c = <-connected:
	}

	// Public ledger table: register for OneOffQuery decode.
	c.RegisterTable(ledgerDef{})

	// --- 1. Ledger -----------------------------------------------------------
	log.Printf("create_ledger name=%s scale=%d kind=Physical", ledgerName, ledgerScale)
	if err := callReducer(ctx, c, reducerErrs, "create_ledger", &createLedgerArgs{
		Name:       ledgerName,
		AssetScale: ledgerScale,
		Kind:       ledgerKindPhysical,
	}); err != nil {
		log.Fatalf("create_ledger: %v", err)
	}

	ledgerID, err := lookupLedgerIDClient(c, ledgerName)
	if err != nil {
		log.Printf("client ledger lookup failed (%v); trying spacetime sql…", err)
		ledgerID, err = lookupLedgerID(*server, *database, ledgerName)
		if err != nil {
			log.Fatalf("resolve ledger id: %v", err)
		}
	}
	log.Printf("ledger id=%d", ledgerID)

	// --- 2. Credit issuer ----------------------------------------------------
	log.Printf("create_account credit address=%s", creditAddress)
	addr := creditAddress
	if err := callReducer(ctx, c, reducerErrs, "create_account", &createAccountArgs{
		LedgerID:  ledgerID,
		Kind:      accountKindCredit,
		Address:   &addr,
		Webhook:   nil,
		IsPrimary: false,
	}); err != nil {
		log.Fatalf("create_account credit: %v", err)
	}

	creditID, err := lookupAccountIDByAddress(*server, *database, creditAddress)
	if err != nil {
		log.Fatalf("resolve credit account: %v", err)
	}
	log.Printf("credit account id=%d address=%s", creditID, creditAddress)

	// --- 3. Debit wallets ----------------------------------------------------
	debitIDs := make([]uint64, 0, debitAccountN)
	for i := 0; i < debitAccountN; i++ {
		log.Printf("create_account debit %d/%d", i+1, debitAccountN)
		before, err := listAccountIDs(*server, *database, ledgerID)
		if err != nil {
			log.Fatalf("list accounts before debit create: %v", err)
		}
		if err := callReducer(ctx, c, reducerErrs, "create_account", &createAccountArgs{
			LedgerID:  ledgerID,
			Kind:      accountKindDebit,
			Address:   nil, // auto-generate
			Webhook:   nil,
			IsPrimary: false,
		}); err != nil {
			log.Fatalf("create_account debit: %v", err)
		}
		id, err := lookupNewAccountID(*server, *database, ledgerID, before)
		if err != nil {
			log.Fatalf("resolve debit account: %v", err)
		}
		debitIDs = append(debitIDs, id)
		log.Printf("  debit account id=%d", id)
	}

	// --- 4. Issue funds (Credit → Debit, posted) ----------------------------
	for i, debitID := range debitIDs {
		key := fmt.Sprintf("seed-hexcoin-issue-%d", i+1)
		log.Printf("issue %d → debit id=%d key=%s", issueAmount, debitID, key)
		if err := callReducer(ctx, c, reducerErrs, "create_transfer", &createTransferArgs{
			SendingAccountID:   creditID,
			ReceivingAccountID: debitID,
			Amount:             issueAmount,
			Memo:               nil,
			IdempotencyKey:     key,
			Pending:            false,
		}); err != nil {
			log.Fatalf("create_transfer issue: %v", err)
		}
	}

	log.Printf("done: ledger=%q id=%d credit=%d (%s) debits=%v issued=%d each",
		ledgerName, ledgerID, creditID, creditAddress, debitIDs, issueAmount)

	_ = c.Disconnect()
	select {
	case <-runDone:
	case <-time.After(500 * time.Millisecond):
	}
}

// ---------------------------------------------------------------------------
// Reducer helper
// ---------------------------------------------------------------------------

// callReducer invokes a reducer then barriers on a public OneOffQuery.
//
// CallReducer itself is fire-and-forget; messages on a connection are processed
// in order, so waiting for a subsequent OneOffQuery ensures the reducer has
// finished (committed or failed) before we continue. No fixed sleeps.
func callReducer(ctx context.Context, c client.DbConnection, errs <-chan error, name string, args bsatn.Serializable) error {
	// Drain stale errors from earlier calls.
	for {
		select {
		case <-errs:
		default:
			goto call
		}
	}
call:
	if err := c.CallReducer(name, args); err != nil {
		return fmt.Errorf("send %s: %w", name, err)
	}

	// Barrier: blocks until the server has processed through this query.
	if _, err := c.OneOffQuery("SELECT * FROM ledger WHERE id = 0"); err != nil {
		// Still check reducer error; query failure may be unrelated.
		select {
		case rerr := <-errs:
			return fmt.Errorf("%s failed: %w", name, rerr)
		default:
			return fmt.Errorf("%s: barrier query failed: %w", name, err)
		}
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	case err := <-errs:
		return fmt.Errorf("%s failed: %w", name, err)
	default:
		return nil
	}
}

// ---------------------------------------------------------------------------
// Ledger lookup via JWT client (public table)
// ---------------------------------------------------------------------------

type ledgerRow struct {
	ID         uint64
	Name       string
	AssetScale uint8
	Kind       uint8
}

type ledgerDef struct{}

func (ledgerDef) TableName() string { return "ledger" }

func (ledgerDef) DecodeRow(r bsatn.Reader) (any, error) {
	id, err := r.GetU64()
	if err != nil {
		return nil, err
	}
	name, err := r.GetString()
	if err != nil {
		return nil, err
	}
	scale, err := r.GetU8()
	if err != nil {
		return nil, err
	}
	kind, err := r.GetSumTag()
	if err != nil {
		return nil, err
	}
	return &ledgerRow{ID: id, Name: name, AssetScale: scale, Kind: kind}, nil
}

func (ledgerDef) EncodeRow(row any) []byte {
	return nil
}

func (ledgerDef) PrimaryKey(row any) any {
	return row.(*ledgerRow).ID
}

func lookupLedgerIDClient(c client.DbConnection, name string) (uint64, error) {
	safe := strings.ReplaceAll(name, "'", "''")
	rows, err := c.OneOffQuery(fmt.Sprintf("SELECT * FROM ledger WHERE name = '%s'", safe))
	if err != nil {
		return 0, err
	}
	for _, raw := range rows {
		row, err := ledgerDef{}.DecodeRow(bsatn.NewReader(raw))
		if err != nil {
			continue
		}
		lr := row.(*ledgerRow)
		if lr.Name == name {
			return lr.ID, nil
		}
	}
	return 0, fmt.Errorf("ledger %q not found via client query", name)
}

// ---------------------------------------------------------------------------
// Owner SQL (private tables) — single shot; CLI must be DB publisher
// ---------------------------------------------------------------------------

func listAccountIDs(server, database string, ledgerID uint64) (map[uint64]struct{}, error) {
	raw, err := spacetimeSQL(server, database, "SELECT id, ledger_id FROM account")
	if err != nil {
		return nil, err
	}
	rows, err := parseSQLRows(raw)
	if err != nil {
		return nil, fmt.Errorf("%w\nraw: %s", err, truncate(string(raw), 500))
	}
	out := make(map[uint64]struct{})
	for _, row := range rows {
		id, ok1 := asUint64(row["id"])
		lid, ok2 := asUint64(row["ledger_id"])
		if ok1 && ok2 && lid == ledgerID {
			out[id] = struct{}{}
		}
	}
	return out, nil
}

func lookupLedgerID(server, database, name string) (uint64, error) {
	safe := strings.ReplaceAll(name, "'", "''")
	raw, err := spacetimeSQL(server, database, fmt.Sprintf("SELECT id, name FROM ledger WHERE name = '%s'", safe))
	if err != nil {
		return 0, err
	}
	rows, err := parseSQLRows(raw)
	if err != nil {
		return 0, fmt.Errorf("%w\nraw: %s", err, truncate(string(raw), 500))
	}
	if len(rows) == 0 {
		return 0, fmt.Errorf("ledger %q not found (query returned no rows after create_ledger)", name)
	}
	id, ok := asUint64(rows[0]["id"])
	if !ok {
		return 0, fmt.Errorf("ledger id not parseable: %v (row=%v)", rows[0]["id"], rows[0])
	}
	return id, nil
}

func lookupAccountIDByAddress(server, database, address string) (uint64, error) {
	safe := strings.ReplaceAll(address, "'", "''")
	raw, err := spacetimeSQL(server, database, fmt.Sprintf("SELECT id, address FROM account WHERE address = '%s'", safe))
	if err != nil {
		return 0, err
	}
	rows, err := parseSQLRows(raw)
	if err != nil {
		return 0, fmt.Errorf("%w\nraw: %s", err, truncate(string(raw), 500))
	}
	if len(rows) == 0 {
		return 0, fmt.Errorf("account address %q not found after create_account (expected row to exist immediately)\nraw: %s",
			address, truncate(string(raw), 500))
	}
	id, ok := asUint64(rows[0]["id"])
	if !ok {
		return 0, fmt.Errorf("account id not parseable: %v (row=%v)\nraw: %s",
			rows[0]["id"], rows[0], truncate(string(raw), 500))
	}
	return id, nil
}

func lookupNewAccountID(server, database string, ledgerID uint64, before map[uint64]struct{}) (uint64, error) {
	after, err := listAccountIDs(server, database, ledgerID)
	if err != nil {
		return 0, err
	}
	for id := range after {
		if _, ok := before[id]; !ok {
			return id, nil
		}
	}
	return 0, fmt.Errorf("no new account on ledger %d after create_account (before=%d after=%d)",
		ledgerID, len(before), len(after))
}

func spacetimeSQL(server, database, query string) ([]byte, error) {
	// Owner identity required for private tables (account). Public ledger also works.
	cmd := exec.Command(
		"spacetime", "sql",
		"-s", server,
		"-y",
		"--format", "json",
		database,
		query,
	)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("spacetime sql: %w\n%s", err, strings.TrimSpace(string(out)))
	}
	// Drop optional WARNING lines before the JSON payload.
	trimmed := strings.TrimSpace(string(out))
	if i := strings.Index(trimmed, "["); i >= 0 {
		trimmed = trimmed[i:]
	} else if i := strings.Index(trimmed, "{"); i >= 0 {
		trimmed = trimmed[i:]
	}
	return []byte(trimmed), nil
}

// sqlResult is one statement result from `spacetime sql --format json`.
type sqlResult struct {
	Schema struct {
		Elements []struct {
			Name json.RawMessage `json:"name"`
		} `json:"elements"`
	} `json:"schema"`
	Rows []json.RawMessage `json:"rows"`
}

// parseSQLRows handles SpacetimeDB `spacetime sql --format json` output:
//
//	[ { "schema": ProductType, "rows": [ ProductValue, ... ] }, ... ]
//
// ProductValue is typically a JSON **array** of field values in schema order
// (not an object), e.g. rows: [[1, "STELOBANK"]].
func parseSQLRows(raw []byte) ([]map[string]any, error) {
	raw = []byte(strings.TrimSpace(string(raw)))
	if len(raw) == 0 {
		return nil, fmt.Errorf("empty sql output")
	}

	dec := func(data []byte, v any) error {
		d := json.NewDecoder(strings.NewReader(string(data)))
		d.UseNumber()
		return d.Decode(v)
	}

	var multi []sqlResult
	if err := dec(raw, &multi); err == nil && len(multi) > 0 {
		return materializeSQLResult(multi[0])
	}

	var single sqlResult
	if err := dec(raw, &single); err == nil && (len(single.Rows) > 0 || len(single.Schema.Elements) > 0) {
		return materializeSQLResult(single)
	}

	// Plain array of objects (fallback).
	var objs []map[string]any
	if err := dec(raw, &objs); err == nil {
		return objs, nil
	}

	return nil, fmt.Errorf("unrecognized sql json: %s", truncate(string(raw), 300))
}

func materializeSQLResult(res sqlResult) ([]map[string]any, error) {
	cols := make([]string, 0, len(res.Schema.Elements))
	for i, el := range res.Schema.Elements {
		name, ok := parseSchemaColumnName(el.Name)
		if !ok || name == "" {
			name = fmt.Sprintf("col%d", i)
		}
		cols = append(cols, name)
	}

	out := make([]map[string]any, 0, len(res.Rows))
	for _, rawRow := range res.Rows {
		row, err := decodeProductRow(rawRow, cols)
		if err != nil {
			return nil, err
		}
		out = append(out, row)
	}
	return out, nil
}

// parseSchemaColumnName accepts {"some":"id"}, {"Some":"id"}, or "id".
func parseSchemaColumnName(raw json.RawMessage) (string, bool) {
	raw = json.RawMessage(strings.TrimSpace(string(raw)))
	if len(raw) == 0 {
		return "", false
	}
	if raw[0] == '"' {
		var s string
		if err := json.Unmarshal(raw, &s); err == nil {
			return s, true
		}
		return "", false
	}
	var opt map[string]any
	if err := json.Unmarshal(raw, &opt); err != nil {
		return "", false
	}
	for _, k := range []string{"some", "Some"} {
		if v, ok := opt[k]; ok {
			if s, ok := v.(string); ok {
				return s, true
			}
		}
	}
	return "", false
}

// decodeProductRow maps a ProductValue (JSON array or object) onto column names.
func decodeProductRow(raw json.RawMessage, cols []string) (map[string]any, error) {
	d := json.NewDecoder(strings.NewReader(string(raw)))
	d.UseNumber()

	// Array form: [id, address, ...]
	var arr []any
	if err := d.Decode(&arr); err == nil {
		m := make(map[string]any, len(arr))
		for i, v := range arr {
			key := fmt.Sprintf("col%d", i)
			if i < len(cols) {
				key = cols[i]
			}
			m[key] = unwrapAlgebraicJSON(v)
		}
		return m, nil
	}

	// Object form: {"id": 1, "address": "..."}
	d = json.NewDecoder(strings.NewReader(string(raw)))
	d.UseNumber()
	var obj map[string]any
	if err := d.Decode(&obj); err == nil {
		for k, v := range obj {
			obj[k] = unwrapAlgebraicJSON(v)
		}
		return obj, nil
	}

	return nil, fmt.Errorf("row is neither array nor object: %s", truncate(string(raw), 120))
}

// unwrapAlgebraicJSON flattens occasional tagged values like {"U64": 1}.
func unwrapAlgebraicJSON(v any) any {
	m, ok := v.(map[string]any)
	if !ok || len(m) != 1 {
		return v
	}
	for _, inner := range m {
		return inner
	}
	return v
}

func asUint64(v any) (uint64, bool) {
	switch t := v.(type) {
	case nil:
		return 0, false
	case float64:
		if t < 0 || t != float64(uint64(t)) {
			return 0, false
		}
		return uint64(t), true
	case json.Number:
		u, err := t.Int64()
		if err != nil || u < 0 {
			// try uint parse
			var uu uint64
			if _, err2 := fmt.Sscanf(t.String(), "%d", &uu); err2 == nil {
				return uu, true
			}
			return 0, false
		}
		return uint64(u), true
	case string:
		var u uint64
		_, err := fmt.Sscanf(t, "%d", &u)
		return u, err == nil
	case int:
		if t < 0 {
			return 0, false
		}
		return uint64(t), true
	case int64:
		if t < 0 {
			return 0, false
		}
		return uint64(t), true
	case uint64:
		return t, true
	default:
		return 0, false
	}
}

func envOr(k, def string) string {
	if v := strings.TrimSpace(os.Getenv(k)); v != "" {
		return v
	}
	return def
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "…"
}

// ---------------------------------------------------------------------------
// BSATN reducer argument types
// ---------------------------------------------------------------------------

type bsatnString string

func (s bsatnString) WriteBsatn(w bsatn.Writer) { w.PutString(string(s)) }

type createLedgerArgs struct {
	Name       string
	AssetScale uint8
	Kind       uint8 // unit enum tag
}

func (a *createLedgerArgs) WriteBsatn(w bsatn.Writer) {
	w.PutString(a.Name)
	w.PutU8(a.AssetScale)
	bsatn.WriteSumUnit(w, a.Kind)
}

type createAccountArgs struct {
	LedgerID  uint64
	Kind      uint8 // unit enum tag
	Address   *string
	Webhook   *string
	IsPrimary bool
}

func (a *createAccountArgs) WriteBsatn(w bsatn.Writer) {
	w.PutU64(a.LedgerID)
	bsatn.WriteSumUnit(w, a.Kind)
	writeOptString(w, a.Address)
	writeOptString(w, a.Webhook)
	w.PutBool(a.IsPrimary)
}

type createTransferArgs struct {
	SendingAccountID   uint64
	ReceivingAccountID uint64
	Amount             uint64
	Memo               *string
	IdempotencyKey     string
	Pending            bool
}

func (a *createTransferArgs) WriteBsatn(w bsatn.Writer) {
	w.PutU64(a.SendingAccountID)
	w.PutU64(a.ReceivingAccountID)
	w.PutU64(a.Amount)
	writeOptString(w, a.Memo)
	w.PutString(a.IdempotencyKey)
	w.PutBool(a.Pending)
}

func writeOptString(w bsatn.Writer, s *string) {
	if s == nil {
		bsatn.WriteOption[bsatnString](w, nil)
		return
	}
	v := bsatnString(*s)
	bsatn.WriteOption(w, &v)
}
