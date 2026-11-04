// Package consent signs the plan summary and cost table the founder confirmed.
// A paid apply is refused unless that exact text was signed and the signature
// is still unused. The operating system signs when biometrics are available.
// Otherwise a local WebAuthn assertion, challenged with this digest, is the
// approval. A yes that does not sign this text is not an approval.
package consent

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"regexp"
	"strconv"
	"strings"
)

const maxText = 1 << 20

var amountRe = regexp.MustCompile(`\$?\d+(?:\.\d+)?`)

// Canonical joins the plan summary and the cost table into the text the
// founder confirms. Newlines are LF. The bytes are otherwise unchanged.
func Canonical(plan, cost string) (string, string, error) {
	plan = normalizeNewlines(plan)
	cost = normalizeNewlines(cost)
	if strings.TrimSpace(plan) == "" || strings.TrimSpace(cost) == "" {
		return "", "", errors.New("plan and cost text are required")
	}
	if len(plan) > maxText || len(cost) > maxText {
		return "", "", errors.New("plan or cost text is too long")
	}
	var b strings.Builder
	b.WriteString("plan\n")
	b.WriteString(plan)
	if !strings.HasSuffix(plan, "\n") {
		b.WriteByte('\n')
	}
	b.WriteString("cost\n")
	b.WriteString(cost)
	if !strings.HasSuffix(cost, "\n") {
		b.WriteByte('\n')
	}
	text := b.String()
	sum := sha256.Sum256([]byte(text))
	return text, hex.EncodeToString(sum[:]), nil
}

// DigestOf returns the hex digest of canonical text.
func DigestOf(canonical string) string {
	sum := sha256.Sum256([]byte(canonical))
	return hex.EncodeToString(sum[:])
}

// Prompt is the biometric reason. It is derived from the digest.
func Prompt(digest string) string {
	return "moxi " + digest
}

// IncursCost reports whether a paid apply must be signed.
// A table with no amounts is treated as paid. An explicit zero is free.
func IncursCost(cost string) bool {
	cost = strings.TrimSpace(normalizeNewlines(cost))
	if cost == "" {
		return true
	}
	switch strings.ToLower(cost) {
	case "none", "no cost", "no charge":
		return false
	}
	matches := amountRe.FindAllString(cost, -1)
	if len(matches) == 0 {
		return true
	}
	for _, match := range matches {
		value, err := strconv.ParseFloat(strings.TrimPrefix(match, "$"), 64)
		if err != nil || value > 0 {
			return true
		}
	}
	return false
}

func normalizeNewlines(s string) string {
	s = strings.ReplaceAll(s, "\r\n", "\n")
	return strings.ReplaceAll(s, "\r", "\n")
}
