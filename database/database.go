package database

import (
	"context"
	"embed"

	"github.com/jackc/pgx/v5/pgxpool"
	_ "github.com/jackc/pgx/v5/stdlib"
	"github.com/pressly/goose/v3"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type Database struct {
	Pool *pgxpool.Pool
	Q    *gensql.Queries
}

//go:embed migrations/*.sql
var embeddedMigrations embed.FS

func RunMigrations(ctx context.Context, getenv func(string) string) error {
	db, err := goose.OpenDBWithDriver(getenv("GOOSE_DRIVER"), getenv("GOOSE_DBSTRING"))
	if err != nil {
		return err
	}

	goose.SetBaseFS(embeddedMigrations)
	if err := goose.SetDialect("postgres"); err != nil {
		return err
	}

	if err := goose.Up(db, "migrations"); err != nil {
		return err
	}

	return nil
}
