package consent

import (
	"context"
	"fmt"
	"strings"
)

// webauthnFallback is the local assertion used when this machine has no
// operating-system biometric. This build reports it unavailable until a
// founder-local authenticator is configured. A generic yes is not an approval.
type webauthnFallback struct{}

func (webauthnFallback) Method() string { return methodWebAuthn }

func (webauthnFallback) Available(context.Context) bool { return false }

func (webauthnFallback) Sign(context.Context, string, string) (Proof, error) {
	return Proof{}, ErrUnavailable
}

func verifyWebAuthn(text string, proof Proof) error {
	if proof.ClientData == "" || proof.AuthData == "" || proof.Signature == "" || proof.CredentialID == "" {
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	if !strings.Contains(proof.ClientData, DigestOf(text)) {
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	return nil
}
