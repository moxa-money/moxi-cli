package cli

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/configfile"
	"github.com/moxa-money/moxi-cli/internal/output"
	"github.com/moxa-money/moxi-cli/internal/setup"
)

// doctorEnv builds the live doctor environment. Tests replace it.
var doctorEnv = setup.LiveEnv

func newSetupCmd(st *State) *cobra.Command {
	var cursorDir string
	var cursorEnable bool

	cmd := &cobra.Command{
		Use:   "setup",
		Short: "Check this machine and scaffold the editor",
	}
	cursorCmd := &cobra.Command{
		Use:   "cursor [dir]",
		Short: "Scaffold editor rules and an example MCP file",
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if cursorEnable {
				if err := st.File.Set("integrations.cursor.enabled", "true"); err != nil {
					return err
				}
				fmt.Fprintln(cmd.OutOrStdout(), "enabled integrations.cursor")
			}
			if !st.File.Integrations.Cursor.Enabled {
				return fmt.Errorf("cursor integration disabled — run: moxi setup cursor --enable")
			}
			dir := cursorDir
			if len(args) > 0 {
				dir = args[0]
			}
			return setup.Cursor(dir, cmd.OutOrStdout())
		},
	}
	cursorCmd.Flags().StringVar(&cursorDir, "dir", "", "target directory (default current directory)")
	cursorCmd.Flags().BoolVar(&cursorEnable, "enable", false, "enable the editor scaffold in founder config and write it")

	cmd.AddCommand(
		cursorCmd,
		&cobra.Command{
			Use:   "doctor",
			Short: "Check local tools and cloud sign-in",
			Args:  cobra.NoArgs,
			RunE: func(cmd *cobra.Command, _ []string) error {
				path, err := configfile.Path()
				if err != nil {
					return err
				}
				_, statErr := os.Stat(path)
				exists := statErr == nil
				rep := setup.Doctor(cmd.Context(), path, exists, doctorEnv())
				if err := writeDoctor(cmd, st, rep); err != nil {
					return err
				}
				if !rep.OK {
					return fmt.Errorf("doctor found issues")
				}
				return nil
			},
		},
	)
	return cmd
}

func writeDoctor(cmd *cobra.Command, st *State, rep setup.Report) error {
	if st.JSON() {
		return output.JSON(cmd.OutOrStdout(), rep)
	}
	fmt.Fprintln(cmd.OutOrStdout(), "moxi doctor")
	rows := make([][]string, 0, len(rep.Checks))
	for _, c := range rep.Checks {
		status := "ok"
		if !c.OK {
			status = "fail"
		}
		rows = append(rows, []string{status, c.Name, c.Detail})
	}
	return output.Table(cmd.OutOrStdout(), []string{"STATUS", "CHECK", "DETAIL"}, rows)
}
