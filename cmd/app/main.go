package main

import (
	"context"
	"fmt"
	"os"

	"github.com/stelofinance/stelofinance/web"
)

func main() {
	ctx := context.Background()
	if err := web.Run(ctx, os.Getenv, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintf(os.Stderr, "%s\n", err)
		os.Exit(1)
	}
}
