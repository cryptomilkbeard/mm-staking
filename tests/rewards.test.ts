import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import { setup, poolPda, stakeVaultPda, rewardVaultPda, createMint, BN, Keypair } from './helpers.ts'

async function initPool(ctx: any) {
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), Keypair.generate().publicKey)
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
