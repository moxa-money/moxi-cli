package main

import (
	"fmt"
	"io"
	"os"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func run(args []string, out io.Writer) error {
	if len(args) == 0 {
		fmt.Fprintln(out, "moxi")
		return nil
	}
	switch args[0] {
	case "config-root":
		dir, err := configroot.Dir()
		if err != nil {
			return err
		}
		fmt.Fprintln(out, dir)
		return nil
	default:
		return fmt.Errorf("unknown command %q", args[0])
	}
}
