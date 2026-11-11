// What the narrator says for a caption or a title card: the same words,
// written for a voice (shared by voice.mjs and record.mjs).
export function speech(text) {
  return text
    .replace(/https?:\/\/\S+/g, "")
    .replace(/(\d+)\.00\b/g, "$1")
    .replace(/\bMCP\b/g, "M C P")
    .replace(/MyNearWallet/g, "My Near Wallet")
    .replace(/TrendPulse/g, "Trend Pulse")
    .replace(/QuickQuote/g, "Quick Quote")
    .replace(/\btestnet\b/g, "test net")
    .replace(/\s+/g, " ")
    .trim();
}
