package main

import (
	"fmt"
	"os"

	"github.com/moxa-money/moxi-cli/internal/cli"
)

func main() {
	if err := cli.Execute(os.Args[1:], os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}
