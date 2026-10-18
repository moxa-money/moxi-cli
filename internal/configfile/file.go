// Package configfile is the founder config file under the moxi config root.
package configfile

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

// File is the on-disk founder config. It names a profile and a context label.
// It does not store a cloud environment, an account id, or a credential.
type File struct {
	Profile      string       `yaml:"profile"`
	Context      string       `yaml:"context"`
	Output       string       `yaml:"output"`
	Profiles     []string     `yaml:"profiles"`
	Integrations Integrations `yaml:"integrations,omitempty"`
}

// Integrations holds opt-in local tool switches. It does not hold a key.
type Integrations struct {
	Cursor CursorIntegration `yaml:"cursor,omitempty"`
}

// CursorIntegration is the editor scaffold switch.
type CursorIntegration struct {
	Enabled bool `yaml:"enabled"`
}

// Default is the file written by config init.
func Default() *File {
	return &File{
		Profile:  "default",
		Context:  "local",
		Output:   "table",
		Profiles: []string{"default"},
	}
}

// Path is the config file path. MOXI_CONFIG_DIR overrides the directory.
func Path() (string, error) {
	dir, err := configroot.Dir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "config.yaml"), nil
}

// Load reads the config file. A missing file returns Default.
func Load() (*File, error) {
	path, err := Path()
	if err != nil {
		return nil, err
	}
	body, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return Default(), nil
		}
		return nil, err
	}
	f := &File{}
	if err := yaml.Unmarshal(body, f); err != nil {
		return nil, fmt.Errorf("config file: %w", err)
	}
	f.normalize()
	return f, nil
}

// Init writes Default when no config file exists.
func Init() (string, error) {
	path, err := Path()
	if err != nil {
		return "", err
	}
	if _, err := os.Stat(path); err == nil {
		return "", fmt.Errorf("config already exists: %s", path)
	} else if !os.IsNotExist(err) {
		return "", err
	}
	if err := Save(Default()); err != nil {
		return "", err
	}
	return path, nil
}

// Save writes f to the config path, creating the directory if needed.
func Save(f *File) error {
	f.normalize()
	path, err := Path()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	body, err := yaml.Marshal(f)
	if err != nil {
		return err
	}
	return os.WriteFile(path, body, 0o600)
}

// Get returns one allowlisted field.
func (f *File) Get(key string) (string, error) {
	switch key {
	case "profile":
		return f.Profile, nil
	case "context":
		return f.Context, nil
	case "output":
		return f.Output, nil
	case "integrations.cursor.enabled":
		return boolString(f.Integrations.Cursor.Enabled), nil
	default:
		return "", fmt.Errorf("unknown config key %q", key)
	}
}

// Set writes one allowlisted field and saves.
func (f *File) Set(key, value string) error {
	switch key {
	case "output":
		if value != "json" && value != "table" {
			return fmt.Errorf("output must be json or table")
		}
		f.Output = value
	case "context":
		label, err := Label(value)
		if err != nil {
			return err
		}
		f.Context = label
	case "integrations.cursor.enabled":
		on, err := parseBool(value)
		if err != nil {
			return err
		}
		f.Integrations.Cursor.Enabled = on
	default:
		return fmt.Errorf("unknown config key %q", key)
	}
	return Save(f)
}

// UseProfile selects a profile that already exists.
func (f *File) UseProfile(name string) error {
	label, err := Label(name)
	if err != nil {
		return err
	}
	if !f.hasProfile(label) {
		return fmt.Errorf("unknown profile %q", label)
	}
	f.Profile = label
	return Save(f)
}

// CreateProfile adds a profile name. It does not select it.
func (f *File) CreateProfile(name string) error {
	label, err := Label(name)
	if err != nil {
		return err
	}
	if f.hasProfile(label) {
		return fmt.Errorf("profile %q already exists", label)
	}
	f.Profiles = append(f.Profiles, label)
	return Save(f)
}

// HasProfile reports whether name is in the profile list.
func (f *File) HasProfile(name string) bool {
	return f.hasProfile(name)
}

func (f *File) hasProfile(name string) bool {
	for _, p := range f.Profiles {
		if p == name {
			return true
		}
	}
	return false
}

func (f *File) normalize() {
	if f.Profile == "" {
		f.Profile = "default"
	}
	if f.Context == "" {
		f.Context = "local"
	}
	if f.Output != "json" {
		f.Output = "table"
	}
	if len(f.Profiles) == 0 {
		f.Profiles = []string{"default"}
	}
	if !f.hasProfile(f.Profile) {
		f.Profiles = append(f.Profiles, f.Profile)
	}
}

// Label checks a profile or context name.
func Label(s string) (string, error) {
	s = strings.TrimSpace(s)
	if s == "" || len(s) > 32 {
		return "", fmt.Errorf("name must be 1 to 32 characters")
	}
	for _, r := range s {
		ok := (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') || r == '-'
		if !ok {
			return "", fmt.Errorf("name %q must be lowercase letters, digits, and hyphens", s)
		}
	}
	if s[0] < 'a' || s[0] > 'z' {
		return "", fmt.Errorf("name %q must start with a letter", s)
	}
	return s, nil
}

func boolString(v bool) string {
	if v {
		return "true"
	}
	return "false"
}

func parseBool(s string) (bool, error) {
	switch s {
	case "true":
		return true, nil
	case "false":
		return false, nil
	default:
		return false, fmt.Errorf("integrations.cursor.enabled: want true or false")
	}
}
