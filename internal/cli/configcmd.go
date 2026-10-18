package cli

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/configfile"
	"github.com/moxa-money/moxi-cli/internal/configroot"
	"github.com/moxa-money/moxi-cli/internal/output"
)

func newConfigCmd(st *State) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "config",
		Short: "Manage founder configuration",
	}
	cmd.AddCommand(
		&cobra.Command{
			Use:   "init",
			Short: "Write the default config file",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				path, err := configfile.Init()
				if err != nil {
					return err
				}
				fmt.Fprintf(cmd.OutOrStdout(), "wrote %s\n", path)
				return nil
			},
		},
		&cobra.Command{
			Use:   "show",
			Short: "Show the active founder config",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				dir, err := configroot.Dir()
				if err != nil {
					return err
				}
				cursor := "false"
				if st.File.Integrations.Cursor.Enabled {
					cursor = "true"
				}
				view := map[string]string{
					"config_dir":     dir,
					"profile":        st.Profile,
					"context":        st.File.Context,
					"output":         st.Output,
					"cursor_enabled": cursor,
				}
				if st.JSON() {
					return output.JSON(cmd.OutOrStdout(), view)
				}
				return output.Table(cmd.OutOrStdout(), []string{"KEY", "VALUE"}, [][]string{
					{"config_dir", dir},
					{"profile", st.Profile},
					{"context", st.File.Context},
					{"output", st.Output},
					{"cursor_enabled", cursor},
				})
			},
		},
		&cobra.Command{
			Use:   "path",
			Short: "Print the config file path",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				path, err := configfile.Path()
				if err != nil {
					return err
				}
				fmt.Fprintln(cmd.OutOrStdout(), path)
				return nil
			},
		},
		&cobra.Command{
			Use:   "set <key> <value>",
			Short: "Set output or context on the config file",
			Args:  cobra.ExactArgs(2),
			RunE: func(cmd *cobra.Command, args []string) error {
				if err := st.File.Set(args[0], args[1]); err != nil {
					return err
				}
				fmt.Fprintf(cmd.OutOrStdout(), "set %s = %s\n", args[0], args[1])
				return nil
			},
		},
		&cobra.Command{
			Use:   "get <key>",
			Short: "Get profile, context, or output",
			Args:  cobra.ExactArgs(1),
			RunE: func(cmd *cobra.Command, args []string) error {
				v, err := st.File.Get(args[0])
				if err != nil {
					return err
				}
				if args[0] == "profile" {
					v = st.Profile
				}
				if args[0] == "output" {
					v = st.Output
				}
				fmt.Fprintln(cmd.OutOrStdout(), v)
				return nil
			},
		},
		newConfigProfileCmd(st),
	)
	return cmd
}

func newConfigProfileCmd(st *State) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "profile",
		Short: "Manage named config profiles",
	}
	cmd.AddCommand(
		&cobra.Command{
			Use:   "list",
			Short: "List profile names",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				if st.JSON() {
					return output.JSON(cmd.OutOrStdout(), map[string]any{
						"active":   st.Profile,
						"profiles": st.File.Profiles,
					})
				}
				for _, name := range st.File.Profiles {
					marker := " "
					if name == st.Profile {
						marker = "*"
					}
					fmt.Fprintf(cmd.OutOrStdout(), "%s %s\n", marker, name)
				}
				return nil
			},
		},
		&cobra.Command{
			Use:   "use <name>",
			Short: "Switch the saved active profile",
			Args:  cobra.ExactArgs(1),
			RunE: func(cmd *cobra.Command, args []string) error {
				if err := st.File.UseProfile(args[0]); err != nil {
					return err
				}
				fmt.Fprintf(cmd.OutOrStdout(), "active profile -> %s\n", args[0])
				return nil
			},
		},
		&cobra.Command{
			Use:   "create <name>",
			Short: "Add a profile name",
			Args:  cobra.ExactArgs(1),
			RunE: func(cmd *cobra.Command, args []string) error {
				if err := st.File.CreateProfile(args[0]); err != nil {
					return err
				}
				fmt.Fprintf(cmd.OutOrStdout(), "created profile %s\n", args[0])
				return nil
			},
		},
	)
	return cmd
}
