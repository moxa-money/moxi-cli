package cli

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/version"
)

func newVersionCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "version",
		Short: "Print this build",
		Long:  "Prints the moxi version and, when the build recorded it, the commit.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			fmt.Fprintln(cmd.OutOrStdout(), version.Line())
			return nil
		},
	}
}
