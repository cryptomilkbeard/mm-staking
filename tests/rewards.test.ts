import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import { setup, poolPda, stakeVaultPda, rewardVaultPda, createMint, BN, Keypair } from './helpers.ts'

async function initPool(ctx: any, keeperKey?: any) {
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  const keeper = keeperKey ?? Keypair.generate().publicKey
  await program.methods.initializePool(new BN(3600), keeper)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  return { stakeMint, pool }
}

test('add_reward registers a slot and creates the reward vault', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)

  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  const acc = await program.account.pool.fetch(pool)
  assert.equal(acc.rewardCount, 1)
  assert.equal(acc.rewards[0].mint.toBase58(), rewardMint.toBase58())
  assert.equal(acc.rewards[0].active, 1)
  assert.equal(acc.rewards[0].duration.toNumber(), 3600) // 0 -> default
})

test('set_reward_active toggles a slot', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)

  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  // deactivate
  await program.methods.setRewardActive(0, false)
    .accounts({ admin: payer.publicKey, pool })
    .rpc()

  let acc = await program.account.pool.fetch(pool)
  assert.equal(acc.rewards[0].active, 0)

  // reactivate
  await program.methods.setRewardActive(0, true)
    .accounts({ admin: payer.publicKey, pool })
    .rpc()

  acc = await program.account.pool.fetch(pool)
  assert.equal(acc.rewards[0].active, 1)
})

test('add_reward rejects duplicate mint (RewardAlreadyExists)', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)

  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  // Attempt to add the same mint again - the vault PDA already exists, and the logic
  // also guards for duplicate mint. Either will cause a rejection.
  await assert.rejects(() =>
    program.methods.addReward(new BN(0))
      .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
      .rpc()
  )
})

test('add_reward rejects non-admin signer (Unauthorized)', async () => {
  const ctx = setup()
  const { svm, program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)

  const stranger = Keypair.generate()
  ;(svm as any).airdrop(stranger.publicKey, 2_000_000_000n)

  await assert.rejects(() =>
    program.methods.addReward(new BN(0))
      .accounts({ admin: stranger.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
      .signers([stranger])
      .rpc()
  )
})

test('add_reward filling all 8 slots succeeds; 9th slot rejects (RewardSlotsFull)', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)

  // Fill all 8 slots
  for (let i = 0; i < 8; i++) {
    const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
    await program.methods.addReward(new BN(0))
      .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
      .rpc()
  }

  const acc = await program.account.pool.fetch(pool)
  assert.equal(acc.rewardCount, 8)

  // 9th must fail
  const extraMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await assert.rejects(() =>
    program.methods.addReward(new BN(0))
      .accounts({ admin: payer.publicKey, pool, rewardMint: extraMint, rewardVault: rewardVaultPda(pool, extraMint) })
      .rpc()
  )
})

test('set_reward_active on empty/never-registered slot rejects (RewardNotFound)', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)

  // No rewards registered yet; slot 0 has default pubkey
  await assert.rejects(() =>
    program.methods.setRewardActive(0, true)
      .accounts({ admin: payer.publicKey, pool })
      .rpc()
  )
})

test('set_reward_active on out-of-range slot (slot=8) rejects (RewardNotFound)', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  // Slot 8 is out of bounds (valid range: 0–7)
  await assert.rejects(() =>
    program.methods.setRewardActive(8, false)
      .accounts({ admin: payer.publicKey, pool })
      .rpc()
  )
})

test('set_reward_active non-admin signer rejects (Unauthorized)', async () => {
  const ctx = setup()
  const { svm, program, provider, payer } = ctx
  const { pool } = await initPool(ctx)
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  const stranger = Keypair.generate()
  ;(svm as any).airdrop(stranger.publicKey, 2_000_000_000n)

  await assert.rejects(() =>
    program.methods.setRewardActive(0, false)
      .accounts({ admin: stranger.publicKey, pool })
      .signers([stranger])
      .rpc()
  )
})
