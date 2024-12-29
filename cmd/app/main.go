package main

import (
	"context"
	"fmt"
	"os"

	"github.com/stelofinance/stelofinance/internal/server"
)

func main() {
	ctx := context.Background()
	if err := server.Run(ctx, os.Getenv, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintf(os.Stderr, "%s\n", err)
		os.Exit(1)
	}
}
