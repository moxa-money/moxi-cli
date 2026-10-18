package cli

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/configroot"
	"github.com/moxa-money/moxi-cli/internal/output"
)

func newCtxCmd(st *State) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "ctx",
		Short: "Show or set the context label",
		Long:  "The context is a local label stored in the founder config file. It is not a cloud environment.",
	}
	cmd.AddCommand(
		&cobra.Command{
			Use:   "show",
			Short: "Show the config directory, profile, and context label",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				dir, err := configroot.Dir()
				if err != nil {
					return err
				}
				view := map[string]string{
					"config_dir": dir,
					"profile":    st.Profile,
					"context":    st.File.Context,
				}
				if st.JSON() {
					return output.JSON(cmd.OutOrStdout(), view)
				}
				return output.Table(cmd.OutOrStdout(), []string{"FIELD", "VALUE"}, [][]string{
					{"config_dir", dir},
					{"profile", st.Profile},
					{"context", st.File.Context},
				})
			},
		},
		&cobra.Command{
			Use:   "use <name>",
			Short: "Save the context label",
			Args:  cobra.ExactArgs(1),
			RunE: func(cmd *cobra.Command, args []string) error {
				if err := st.File.Set("context", args[0]); err != nil {
					return err
				}
				fmt.Fprintf(cmd.OutOrStdout(), "active context -> %s\n", args[0])
				return nil
			},
		},
	)
	return cmd
}
