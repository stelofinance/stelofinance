package database

import (
	"context"
	"database/sql"
	"embed"
	"errors"

	"github.com/pressly/goose/v3"
	"github.com/stelofinance/stelofinance/database/gensql"
	// _ "turso.tech/database/tursogo"
	_ "modernc.org/sqlite"
)

type Database struct {
	pool *sql.DB
	Q    *gensql.Queries
}

type Option string

const (
	// Enable Foreign Key Constraints
	WithForeignKeys Option = "PRAGMA foreign_keys = ON;"
)

func New(pool *sql.DB, q *gensql.Queries) *Database {
	return &Database{
		pool: pool,
		Q:    q,
	}
}

// Conn is a wrapper for [sql.(DB).Conn] (so you must still call [Conn.Close] after use).
//
// opts can be passed to enable database configuration PRAGMAs on the returned [Conn].
func (d *Database) Conn(ctx context.Context, opts ...Option) (*sql.Conn, error) {
	conn, err := d.pool.Conn(ctx)
	if err != nil {
		return nil, err
	}
	if len(opts) == 0 {
		return conn, nil
	}

	statement := ""
	for _, opt := range opts {
		statement += string(opt)
	}
	_, err = conn.ExecContext(ctx, statement)
	if err != nil {
		return nil, err
	}
	return conn, nil

}

type QueriesTx struct {
	conn *sql.Conn
	tx   *sql.Tx
}

func (qtx *QueriesTx) Q() *gensql.Queries {
	return gensql.New(qtx.tx)
}

func (qtx *QueriesTx) Commit() error {
	return qtx.tx.Commit()
}

// Cleanup will rollback the underlying transaction (if it wasn't already committed)
// and return the underlying connection back to the pool.
//
// Generally this should be deferred right after calling [Database.QTx]
func (qtx *QueriesTx) Cleanup() error {
	err1 := qtx.tx.Rollback()
	err2 := qtx.conn.Close()

	return errors.Join(err1, err2)
}

// QTx is a wrapper that gets a db connection, applies any opts,
// starts a transaction, and returns it.
func (d *Database) QTx(ctx context.Context, opts ...Option) (*QueriesTx, error) {
	conn, err := d.Conn(ctx, opts...)
	if err != nil {
		return nil, err
	}

	tx, err := conn.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}

	return &QueriesTx{
		conn: conn,
		tx:   tx,
	}, nil
}

//go:embed migrations/*.sql
var embeddedMigrations embed.FS

func RunMigrations(ctx context.Context, getenv func(string) string) error {
	// Connect up DB
	dbConn, err := sql.Open("sqlite", getenv("GOOSE_DBSTRING"))
	if err != nil {
		return err
	}

	goose.SetBaseFS(embeddedMigrations)

	if err := goose.SetDialect(getenv("GOOSE_DRIVER")); err != nil {
		return err
	}

	if err := goose.Up(dbConn, "migrations"); err != nil {
		return err
	}

	return dbConn.Close()
}
