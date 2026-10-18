package cli

import (
	"fmt"
	"strings"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/output"
)

// UsageDoc is the agent contract for the commands registered in this build.
type UsageDoc struct {
	Summary  string         `json:"summary"`
	Requires []string       `json:"requires"`
	Refuses  string         `json:"refuses"`
	Commands []UsageCommand `json:"commands"`
}

// UsageCommand is one example in the contract.
type UsageCommand struct {
	Command string `json:"command"`
	Example string `json:"example"`
}

func localUsage() UsageDoc {
	return UsageDoc{
		Summary: "Founder-local commands. Settings live in the config directory.",
		Requires: []string{
			"--output json for a machine-readable result",
			"MOXI_CONFIG_DIR when the config root is not the default",
		},
		Refuses: "an environment flag and a control-plane client",
		Commands: []UsageCommand{
			{Command: "config path", Example: "moxi config path"},
			{Command: "config init", Example: "moxi config init"},
			{Command: "config show", Example: "moxi config show --output json"},
			{Command: "ctx show", Example: "moxi ctx show"},
			{Command: "version", Example: "moxi version"},
			{Command: "help tree", Example: "moxi help tree"},
		},
	}
}

func formatUsage(doc UsageDoc) string {
	var b strings.Builder
	fmt.Fprintf(&b, "%s\n\nRequires\n", doc.Summary)
	for _, line := range doc.Requires {
		fmt.Fprintf(&b, "  %s\n", line)
	}
	fmt.Fprintf(&b, "\nRefuses  %s\n\n", doc.Refuses)
	fmt.Fprintf(&b, "%-16s  %s\n", "COMMAND", "EXAMPLE")
	for _, command := range doc.Commands {
		fmt.Fprintf(&b, "%-16s  %s\n", command.Command, command.Example)
	}
	return b.String()
}

func newUsageCmd(st *State) *cobra.Command {
	return &cobra.Command{
		Use:     "usage",
		Short:   "Show the flag contract",
		Long:    "Print the contract for the founder-local commands.\nPass --output json for the machine-readable list.",
		Example: "  moxi usage\n  moxi usage --output json",
		RunE: func(cmd *cobra.Command, _ []string) error {
			doc := localUsage()
			if st.JSON() {
				return output.JSON(cmd.OutOrStdout(), doc)
			}
			fmt.Fprint(cmd.OutOrStdout(), formatUsage(doc))
			return nil
		},
	}
}
