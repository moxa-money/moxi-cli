//go:build darwin

package consent

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
)

func (b platformBiometric) Available(ctx context.Context) bool {
	res, err := b.call(ctx, touchReq{Op: "available"})
	return err == nil && res.Error == ""
}

func (b platformBiometric) Sign(ctx context.Context, text, digest string) (Proof, error) {
	res, err := b.call(ctx, touchReq{Op: "sign", Text: text, Prompt: Prompt(digest)})
	if err != nil {
		return Proof{}, ErrUnavailable
	}
	if res.Error != "" {
		return Proof{}, helperError(res.Error)
	}
	if res.PublicKey == "" || res.Signature == "" {
		return Proof{}, ErrUnavailable
	}
	return Proof{Method: methodBiometric, PublicKey: res.PublicKey, Signature: res.Signature}, nil
}

type touchReq struct {
	Op     string `json:"op"`
	Text   string `json:"text,omitempty"`
	Prompt string `json:"prompt,omitempty"`
}

type touchRes struct {
	PublicKey string `json:"publicKey"`
	Signature string `json:"signature"`
	Error     string `json:"error"`
}

func (platformBiometric) call(ctx context.Context, req touchReq) (touchRes, error) {
	swift, err := exec.LookPath("swift")
	if err != nil {
		return touchRes{}, ErrUnavailable
	}
	src := filepath.Join("internal", "consent", "touch.swift")
	if _, err := os.Stat(src); err != nil {
		return touchRes{}, ErrUnavailable
	}
	body, err := json.Marshal(req)
	if err != nil {
		return touchRes{}, err
	}
	cmd := exec.CommandContext(ctx, swift, src)
	cmd.Stdin = bytes.NewReader(body)
	out, err := cmd.Output()
	if err != nil {
		return touchRes{}, ErrUnavailable
	}
	var res touchRes
	if err := json.Unmarshal(out, &res); err != nil {
		return touchRes{}, ErrUnavailable
	}
	return res, nil
}
