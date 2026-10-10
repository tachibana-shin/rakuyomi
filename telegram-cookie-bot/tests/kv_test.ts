import { strict as assert } from "node:assert"
import { test } from "node:test"
import { useTestEnv } from "./setup.ts"

import {
  createPairingCode,
  getPairingPendingCount,
  getPairingStatus,
  removePairingByDevice,
  resolvePairingCode,
} from "../src/kv.ts"

useTestEnv()

test("pairing — full lifecycle: create -> resolve -> status -> remove", async () => {
  const code = "TESTCODE"

  await createPairingCode(code)

  const unpaired = await getPairingStatus(code)
  assert.strictEqual(unpaired.paired, false)

  const resolved = await resolvePairingCode(code, 100, "test_device")
  assert.ok(typeof resolved === "string")
  assert.ok(resolved!.length > 0)

  const paired = await getPairingStatus(code)
  assert.strictEqual(paired.paired, true)
  assert.strictEqual(paired.chat_id, 100)
  assert.strictEqual(paired.device_name, "test_device")

  const removed = await removePairingByDevice(100, "test_device")
  assert.strictEqual(removed, true)

  const afterRemove = await getPairingStatus(code)
  assert.strictEqual(afterRemove.paired, false)
})

test("pairing — resolve non-existent code returns null", async () => {
  const result = await resolvePairingCode("NONEXISTENT", 999, "ghost_device")
  assert.strictEqual(result, null)
})

test("pairing — status for non-existent code returns unpaired", async () => {
  const status = await getPairingStatus("NONEXISTENT")
  assert.strictEqual(status.paired, false)
})

test("pairing — remove non-existent device returns false", async () => {
  const result = await removePairingByDevice(999, "ghost_device")
  assert.strictEqual(result, false)
})

test("pairing — createPairingCode increments pending count", async () => {
  const before = await getPairingPendingCount()
  await createPairingCode("PENDING1")
  await createPairingCode("PENDING2")
  const after = await getPairingPendingCount()
  assert.strictEqual(after, before + 2)
})
