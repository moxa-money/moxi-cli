package consent

import (
	"strings"
)

type platformBiometric struct{}

func (platformBiometric) Method() string { return methodBiometric }

func helperError(msg string) error {
	switch {
	case strings.Contains(msg, "cancel"):
		return ErrCancelled
	case msg == "" || strings.Contains(msg, "unavailable"):
		return ErrUnavailable
	default:
		return ErrUnavailable
	}
}
