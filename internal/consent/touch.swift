import CryptoKit
import Foundation
import LocalAuthentication
import Security

struct InReq: Decodable {
    let op: String
    let text: String?
    let prompt: String?
}

struct OutRes: Encodable {
    var publicKey: String = ""
    var signature: String = ""
    var error: String = ""
}

func emit(_ res: OutRes) -> Never {
    let data = (try? JSONEncoder().encode(res)) ?? Data()
    if let line = String(data: data, encoding: .utf8) {
        print(line)
    }
    exit(0)
}

let raw = FileHandle.standardInput.readDataToEndOfFile()
guard let req = try? JSONDecoder().decode(InReq.self, from: raw) else {
    emit(OutRes(error: "unavailable"))
}

let context = LAContext()
var laError: NSError?
let biometrics = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &laError)
if req.op == "available" {
    emit(OutRes(error: biometrics ? "" : "unavailable"))
}
if !biometrics {
    emit(OutRes(error: "unavailable"))
}
guard let text = req.text, !text.isEmpty else {
    emit(OutRes(error: "unavailable"))
}
context.localizedReason = req.prompt ?? "moxi"

var cfError: Unmanaged<CFError>?
guard let access = SecAccessControlCreateWithFlags(
    nil,
    kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    [.privateKeyUsage, .biometryCurrentSet],
    &cfError
) else {
    emit(OutRes(error: "unavailable"))
}

do {
    let key = try SecureEnclave.P256.Signing.PrivateKey(
        accessControl: access,
        authenticationContext: context
    )
    let sig = try key.signature(for: Data(text.utf8))
    emit(OutRes(
        publicKey: key.publicKey.x963Representation.base64EncodedString(),
        signature: sig.derRepresentation.base64EncodedString()
    ))
} catch {
    let msg = error.localizedDescription.lowercased()
    if msg.contains("cancel") {
        emit(OutRes(error: "cancelled"))
    }
    emit(OutRes(error: "unavailable"))
}
