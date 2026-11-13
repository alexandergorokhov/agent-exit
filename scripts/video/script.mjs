// The narration: what the voice says, shot by shot. Captions on screen are
// shorter; record.mjs pairs them. voice.mjs turns each line into a wav.
// One persona: Alice built Polyglot and wants to sell it; Bob buys; the viewer can invest (Dave is the recorded investor).
export const LINES = {
  hook: "Agents now earn money doing real work, so people want to buy them, like small online businesses. But a buyer faces two problems. First, revenue can be faked. This agent took in about five hundred dollars, and one hundred forty of it came from its own owner.",
  problem: "Second, the seller can keep a secret key, and take the agent back after the sale.",
  market: "Agent Exit solves both, on NEAR. Each agent is its own account. Its contract counts only money from real customers. And a sale happens in one transaction that deletes the seller's keys.",
  cheat: "Quick Quote hides a secret key. The contract refuses to sell it, and a buyer who pays anyway gets the money back.",
  sold: "Polyglot sold in one transaction. Alice's key was deleted, and all six of her attempts to get back in failed.",
  evidence: "The page re-checks every recorded transaction on test net.",
  invest: "Or buy a share of future earnings: two and a half percent of Lingo Live, for twenty five test dollars.",
  mcp: "Agents can buy too. Our server makes them scan before they pay.",
  playInvest: "Try it in your browser.",
  playPay: "Dave's share is paid from every customer payment.",
  playBuy: "Bob buys the agent, and part of the price waits for real customers.",
  breakin: "Alice tries to get back in. Every attempt is refused.",
  whyNew: "Code, not promises, locks the seller out.",
  close: "Agent Exit. Buy agents by what they really earn.",
};
