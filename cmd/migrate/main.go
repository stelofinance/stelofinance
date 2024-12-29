package main

import (
	"context"
	"fmt"
	"os"

	"github.com/stelofinance/stelofinance/database"
)

func main() {
	ctx := context.Background()
	if err := database.RunMigrations(ctx, os.Getenv); err != nil {
		fmt.Fprintf(os.Stderr, "%s\n", err)
		os.Exit(1)
	}
}
