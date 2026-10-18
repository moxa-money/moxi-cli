package cli

import (
	"fmt"
	"sort"
	"strings"

	"github.com/spf13/cobra"
)

func newHelpCmd() *cobra.Command {
	help := &cobra.Command{
		Use:   "help [command]",
		Short: "Help about any command",
		Long:  "Help for one command. help tree prints the registered command tree.",
		Run: func(c *cobra.Command, args []string) {
			cmd, _, err := c.Root().Find(args)
			if cmd == nil || err != nil {
				c.Printf("Unknown help topic %#q\n", args)
				cobra.CheckErr(c.Root().Usage())
				return
			}
			cmd.InitDefaultHelpFlag()
			cmd.InitDefaultVersionFlag()
			cobra.CheckErr(cmd.Help())
		},
	}
	help.AddCommand(&cobra.Command{
		Use:   "tree",
		Short: "Print the command tree",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			fmt.Fprint(cmd.OutOrStdout(), renderTree(cmd.Root()))
			return nil
		},
	})
	return help
}

// CommandTree is the text printed by help tree.
func CommandTree() (string, error) {
	root, err := newRoot()
	if err != nil {
		return "", err
	}
	root.InitDefaultHelpCmd()
	return renderTree(root), nil
}

type treeNode struct {
	name string
	desc string
	kids []treeNode
}

func renderTree(root *cobra.Command) string {
	var b strings.Builder
	fmt.Fprintf(&b, "%s  %s\n", root.Name(), oneLine(root.Short))
	writeKids(&b, childNodes(root), "")
	return b.String()
}

func childNodes(cmd *cobra.Command) []treeNode {
	subs := append([]*cobra.Command{}, cmd.Commands()...)
	sort.Slice(subs, func(i, j int) bool { return subs[i].Name() < subs[j].Name() })
	nodes := make([]treeNode, 0, len(subs))
	for _, sub := range subs {
		if !includeCommand(sub) {
			continue
		}
		nodes = append(nodes, treeNode{
			name: sub.Name(),
			desc: oneLine(sub.Short),
			kids: childNodes(sub),
		})
	}
	return nodes
}

func includeCommand(cmd *cobra.Command) bool {
	if cmd.Hidden || cmd.Deprecated != "" || cmd.Name() == "__complete" || cmd.Name() == "completion" {
		return false
	}
	if cmd.Name() == "help" {
		return true
	}
	return cmd.IsAvailableCommand()
}

func writeKids(b *strings.Builder, nodes []treeNode, prefix string) {
	for i, n := range nodes {
		last := i == len(nodes)-1
		branch := "├── "
		next := prefix + "│   "
		if last {
			branch = "└── "
			next = prefix + "    "
		}
		fmt.Fprintf(b, "%s%s%s  %s\n", prefix, branch, n.name, n.desc)
		writeKids(b, n.kids, next)
	}
}

func oneLine(s string) string {
	s = strings.TrimSpace(s)
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		s = s[:i]
	}
	return s
}
