// Package cli is the moxi command tree.
package cli

import (
	"fmt"
	"io"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/configfile"
	"github.com/moxa-money/moxi-cli/internal/version"
)

// State is the resolved invocation. Flags override the config file.
type State struct {
	File    *configfile.File
	Output  string
	Profile string
	NoInput bool
	Verbose bool
}

func (s *State) JSON() bool { return s.Output == "json" }

// Execute runs one invocation. args excludes the program name.
func Execute(args []string, stdout, stderr io.Writer) error {
	root, err := newRoot()
	if err != nil {
		return err
	}
	root.SetOut(stdout)
	root.SetErr(stderr)
	root.SetArgs(args)
	return root.Execute()
}

func newRoot() (*cobra.Command, error) {
	st := &State{}
	var flagOutput, flagProfile string
	var flagNoInput, flagVerbose bool

	root := &cobra.Command{
		Use:           "moxi",
		Short:         "Founder CLI",
		Long:          "Founder shell: local config, the ledger, the desk, and setup checks.\nAgents pass --output json. usage lists the flags they follow.\nThe ledger binary is comms, built from ledger/.",
		Version:       version.Line(),
		SilenceUsage:  true,
		SilenceErrors: true,
		PersistentPreRunE: func(_ *cobra.Command, _ []string) error {
			f, err := configfile.Load()
			if err != nil {
				return err
			}
			st.File = f
			st.Output = f.Output
			st.Profile = f.Profile
			st.NoInput = flagNoInput
			st.Verbose = flagVerbose
			if flagOutput != "" {
				if flagOutput != "json" && flagOutput != "table" {
					return fmt.Errorf("output must be json or table")
				}
				st.Output = flagOutput
			}
			if flagProfile != "" {
				label, err := configfile.Label(flagProfile)
				if err != nil {
					return err
				}
				if !f.HasProfile(label) {
					return fmt.Errorf("unknown profile %q", label)
				}
				st.Profile = label
			}
			return nil
		},
	}
	root.SetVersionTemplate("{{.Version}}\n")
	root.CompletionOptions.DisableDefaultCmd = true

	pf := root.PersistentFlags()
	pf.StringVar(&flagOutput, "output", "", "output mode: json|table")
	pf.StringVar(&flagProfile, "profile", "", "config profile (default from the founder config file)")
	pf.BoolVar(&flagNoInput, "no-input", false, "never prompt")
	pf.BoolVar(&flagVerbose, "verbose", false, "verbose local logging")

	root.AddGroup(
		&cobra.Group{ID: "local", Title: "This machine:"},
		&cobra.Group{ID: "ledger", Title: "Ledger:"},
		&cobra.Group{ID: "screen", Title: "Screen:"},
		&cobra.Group{ID: "contract", Title: "Agent contract:"},
	)
	root.SetHelpCommand(newHelpCmd())
	root.AddCommand(
		newConfigCmd(st),
		newCtxCmd(st),
		newSetupCmd(st),
		newCommsCmd(),
		newDeskCmd(st),
		newDashCmd(st),
		newUsageCmd(st),
		newVersionCmd(),
	)
	groups := map[string]string{
		"config":  "local",
		"ctx":     "local",
		"setup":   "local",
		"version": "local",
		"comms":   "ledger",
		"desk":    "screen",
		"dash":    "screen",
		"usage":   "contract",
	}
	for _, cmd := range root.Commands() {
		if id, ok := groups[cmd.Name()]; ok {
			cmd.GroupID = id
		}
	}
	return root, nil
}
