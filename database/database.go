package database

import (
	"context"
	"database/sql"
	"embed"

	"github.com/pressly/goose/v3"
	"github.com/stelofinance/stelofinance/database/gensql"
	_ "modernc.org/sqlite"
)

type Database struct {
	Pool *sql.DB
	Q    *gensql.Queries
}

func New(pool *sql.DB, q *gensql.Queries) *Database {
	return &Database{
		Pool: pool,
		Q:    q,
	}
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
