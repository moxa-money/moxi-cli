package consent

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"os"
	"path/filepath"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

const (
	methodNone      = "none"
	methodBiometric = "biometric"
	methodWebAuthn  = "webauthn"
)

// ErrUnavailable means this machine has no operating-system biometric.
// ErrCancelled means the founder dismissed the approval.
// ErrRefused means apply must not continue.
var (
	ErrUnavailable = errors.New("biometrics unavailable")
	ErrCancelled   = errors.New("approval cancelled")
	ErrRefused     = errors.New("paid apply refused")
)

// Result is the approval the command prints. It does not include a signature.
type Result struct {
	Text       string `json:"text"`
	Digest     string `json:"digest"`
	IncursCost bool   `json:"incurs_cost"`
	Method     string `json:"method"`
	Status     string `json:"status"`
}

// Signer approves one canonical payload.
type Signer interface {
	Method() string
	Available(ctx context.Context) bool
	Sign(ctx context.Context, canonical, digest string) (Proof, error)
}

// Proof is the signature material stored for one approval.
type Proof struct {
	Method       string `json:"method"`
	PublicKey    string `json:"publicKey,omitempty"`
	Signature    string `json:"signature,omitempty"`
	ClientData   string `json:"clientData,omitempty"`
	AuthData     string `json:"authData,omitempty"`
	CredentialID string `json:"credentialId,omitempty"`
}

type grant struct {
	Digest string `json:"digest"`
	Proof
	Used bool `json:"used"`
}

// Approve canonicalizes the plan and the cost table and, when the table
// incurs cost, stores a signature over that exact text.
func Approve(ctx context.Context, plan, cost string) (Result, error) {
	return ApproveWith(ctx, plan, cost, platformBiometric{}, webauthnFallback{})
}

// ApproveWith is Approve with the two signers chosen by the caller.
func ApproveWith(ctx context.Context, plan, cost string, bio, web Signer) (Result, error) {
	text, digest, err := Canonical(plan, cost)
	if err != nil {
		return Result{}, err
	}
	result := Result{Text: text, Digest: digest, IncursCost: IncursCost(cost), Status: "approved"}
	g := grant{Digest: digest}
	if !result.IncursCost {
		g.Method = methodNone
		result.Method = methodNone
		return result, writeGrant(g)
	}
	proof, err := signPaid(ctx, text, digest, bio, web)
	if err != nil {
		return Result{}, err
	}
	g.Proof = proof
	result.Method = proof.Method
	if err := verifyProof(text, g); err != nil {
		return Result{}, err
	}
	return result, writeGrant(g)
}

// Consume checks the stored approval for this exact plan and cost text and
// marks it used. A second call for the same text is refused.
func Consume(plan, cost string) (Result, error) {
	text, digest, err := Canonical(plan, cost)
	if err != nil {
		return Result{}, err
	}
	g, err := readGrant(digest)
	if err != nil {
		return Result{}, err
	}
	if g.Used {
		return Result{}, fmt.Errorf("%w: approval already used", ErrRefused)
	}
	if err := verifyProof(text, g); err != nil {
		return Result{}, err
	}
	if err := markUsed(digest, g); err != nil {
		return Result{}, err
	}
	return Result{
		Text:       text,
		Digest:     digest,
		IncursCost: IncursCost(cost),
		Method:     g.Method,
		Status:     "allowed",
	}, nil
}

func signPaid(ctx context.Context, text, digest string, bio, web Signer) (Proof, error) {
	if bio != nil && bio.Available(ctx) {
		proof, err := bio.Sign(ctx, text, digest)
		if err == nil {
			proof.Method = methodBiometric
			return proof, nil
		}
		if !errors.Is(err, ErrUnavailable) {
			return Proof{}, err
		}
	}
	if web == nil || !web.Available(ctx) {
		return Proof{}, fmt.Errorf("%w: a signature bound to this plan is required", ErrRefused)
	}
	proof, err := web.Sign(ctx, text, digest)
	if err != nil {
		return Proof{}, err
	}
	proof.Method = methodWebAuthn
	return proof, nil
}

func verifyProof(text string, g grant) error {
	if g.Digest != DigestOf(text) {
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	paid := methodNone != g.Method
	switch g.Method {
	case methodNone:
		return nil
	case methodBiometric:
		if err := verifyBiometric(g.PublicKey, text, g.Signature); err != nil {
			return err
		}
		return nil
	case methodWebAuthn:
		return verifyWebAuthn(text, g.Proof)
	default:
		if paid {
			return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
		}
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
}

func verifyBiometric(publicKey, canonical, signature string) error {
	raw, sig, err := decodeP256(publicKey, signature)
	if err != nil {
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	pub := ecdsa.PublicKey{
		Curve: elliptic.P256(),
		X:     new(big.Int).SetBytes(raw[1:33]),
		Y:     new(big.Int).SetBytes(raw[33:65]),
	}
	sum := sha256.Sum256([]byte(canonical))
	if !ecdsa.VerifyASN1(&pub, sum[:], sig) {
		return fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	return nil
}

func decodeP256(publicKey, signature string) ([]byte, []byte, error) {
	raw, err := base64.StdEncoding.DecodeString(publicKey)
	if err != nil || len(raw) != 65 || raw[0] != 4 {
		return nil, nil, errors.New("key")
	}
	sig, err := base64.StdEncoding.DecodeString(signature)
	if err != nil || len(sig) == 0 {
		return nil, nil, errors.New("signature")
	}
	return raw, sig, nil
}

func writeGrant(g grant) error {
	path, err := grantPath(g.Digest)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	body, err := json.MarshalIndent(g, "", "  ")
	if err != nil {
		return err
	}
	body = append(body, '\n')
	return os.WriteFile(path, body, 0o600)
}

func readGrant(digest string) (grant, error) {
	path, err := grantPath(digest)
	if err != nil {
		return grant{}, err
	}
	body, err := os.ReadFile(path)
	if err != nil {
		return grant{}, fmt.Errorf("%w: no approval for this plan", ErrRefused)
	}
	var g grant
	if err := json.Unmarshal(body, &g); err != nil {
		return grant{}, fmt.Errorf("%w: no approval for this plan", ErrRefused)
	}
	if g.Digest != digest {
		return grant{}, fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	return g, nil
}

func markUsed(digest string, g grant) error {
	dir, err := configroot.Dir()
	if err != nil {
		return err
	}
	marker := filepath.Join(dir, "consent", "used", digest)
	if err := os.MkdirAll(filepath.Dir(marker), 0o700); err != nil {
		return err
	}
	f, err := os.OpenFile(marker, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return fmt.Errorf("%w: approval already used", ErrRefused)
	}
	_, _ = f.Write([]byte(g.Signature))
	_ = f.Close()
	g.Used = true
	return writeGrant(g)
}

func grantPath(digest string) (string, error) {
	if len(digest) != 64 {
		return "", fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	if _, err := hex.DecodeString(digest); err != nil {
		return "", fmt.Errorf("%w: signature is not bound to this plan", ErrRefused)
	}
	dir, err := configroot.Dir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "consent", digest+".json"), nil
}
