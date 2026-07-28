function decodeMinisignText(value, label, { outerBase64 = false } = {}) {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label}为空`)
  const text = outerBase64 ? Buffer.from(value.trim(), "base64").toString("utf8") : value.trim()
  if (!text.startsWith("untrusted comment:")) {
    throw new Error(`${label}不是 minisign 文本`)
  }
  return text
}

function firstPacket(text, label) {
  const line = text
    .split(/\r?\n/)
    .map((entry) => entry.trim())
    .find((entry) => entry && !entry.endsWith("comment:") && !entry.includes("comment:"))
  if (!line || !/^[A-Za-z0-9+/]+={0,2}$/.test(line)) {
    throw new Error(`${label}缺少 minisign 数据包`)
  }
  const packet = Buffer.from(line, "base64")
  if (packet.length < 10) throw new Error(`${label}的 minisign 数据包过短`)
  return packet
}

export function minisignPublicKeyId(publicKey) {
  const text = decodeMinisignText(publicKey, "updater 公钥", { outerBase64: true })
  return firstPacket(text, "updater 公钥").subarray(2, 10).toString("hex")
}

export function minisignSignatureKeyId(signature) {
  const text = decodeMinisignText(signature, "updater 签名", { outerBase64: true })
  return firstPacket(text, "updater 签名").subarray(2, 10).toString("hex")
}

export function assertSignatureMatchesPublicKey(signature, publicKey) {
  const signatureId = minisignSignatureKeyId(signature)
  const publicKeyId = minisignPublicKeyId(publicKey)
  if (signatureId !== publicKeyId) {
    throw new Error(`updater 私钥与配置公钥不匹配 (${signatureId} != ${publicKeyId})`)
  }
  return publicKeyId
}
