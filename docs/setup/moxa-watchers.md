# Moxa Watchers

Create this code-signing identity once on a Mac. The private key is created in the login keychain and stays there. Setup does not create the certificate and does not know the login password.

## Create

1. Open Keychain Access.
2. Choose Keychain Access → Certificate Assistant → Create a Certificate.
3. Name: `Moxa Watchers`. Identity Type: Self Signed Root. Certificate Type: Code Signing.
4. Turn on Let me override defaults, then continue.
5. Set only these fields when the assistant asks for them:
   - Validity: 3650 days
   - Algorithm: RSA
   - Key size: 2048
   - Keychain: login
6. Key Usage Extension, Extended Key Usage Extension, and Basic Constraints Extension are not specified. On each of those screens, change nothing, then continue.
7. On any other screen, change nothing, then continue.
8. Create the certificate, then click Done.
9. If macOS asks to allow `/usr/bin/codesign` to use the private key, approve that prompt. Touch ID or the login password is the expected prompt. Choose Always Allow for `codesign`. The prompt can appear while the certificate is created or the first time the key is used.

## Confirm

```bash
security find-identity -v -p codesigning
```

The list includes `Moxa Watchers`. That is the identity.

## Leave the key in login

Do not export the private key. Do not write a `.p12` or a PEM. Do not run `security export`.
