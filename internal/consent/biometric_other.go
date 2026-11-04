//go:build !darwin

package consent

import "context"

func (platformBiometric) Available(context.Context) bool { return false }

func (platformBiometric) Sign(context.Context, string, string) (Proof, error) {
	return Proof{}, ErrUnavailable
}
