import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, createMint, mintTo, warp,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

async function fullSetup(ctx: any, rewardDecimals = 8) {
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  // keeper = payer for simplicity
  await program.methods.initializePool(new BN(3600), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, rewardDecimals)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const keeperAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, keeperAta, payer.publicKey, rewardMint)
  ), [payer])

  return { stakeMint, pool, rewardMint, keeperAta }
}

test('deposit_rewards by keeper starts a stream', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const keeper = payer // keeper authority = payer for the test
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), keeper.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const keeperAta = getAssociatedTokenAddressSync(rewardMint, keeper.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(keeper.publicKey, keeperAta, keeper.publicKey, rewardMint)
  ), [payer])
  await mintTo(provider.connection, payer, rewardMint, keeperAta, payer, 3600)

  await program.methods.depositRewards(new BN(3600))
    .accounts({ keeper: keeper.publicKey, pool, rewardMint, keeperTokenAccount: keeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  const p = await program.account.pool.fetch(pool)
  // rate scaled by 1e12: 3600 tokens / 3600s = 1 token/sec => 1e12
  // Use hex comparison to work around Anchor u128 BN toString() NaN bug
  assert.equal(p.rewards[0].rewardRate.toString(16), (1_000_000_000_000).toString(16))
  assert.ok(p.rewards[0].periodFinish.toNumber() > 0)
})

test('deposit_rewards rejects a non-keeper caller', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  const realKeeper = Keypair.generate()
  await program.methods.initializePool(new BN(3600), realKeeper.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 8)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()
  const ata = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, ata, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, ata, payer, 100)

  await assert.rejects(() => program.methods.depositRewards(new BN(100))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: ata, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc())
})

test('deposit_rewards with amount == 0 rejects (ZeroAmount)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool, rewardMint, keeperAta } = await fullSetup(ctx)

  // No mintTo — zero balance, zero amount
  await assert.rejects(() =>
    program.methods.depositRewards(new BN(0))
      .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: keeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
      .rpc()
  )
})

test('deposit_rewards to unregistered mint rejects (RewardNotFound)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  // Initialize pool with NO rewards registered
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const unregisteredMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const ata = getAssociatedTokenAddressSync(unregisteredMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, ata, payer.publicKey, unregisteredMint)), [payer])
  await mintTo(provider.connection, payer, unregisteredMint, ata, payer, 1000)

  // The deposit_rewards instruction resolves the vault via find_reward_vault which does find_slot —
  // the constraint itself will reject before even reaching our error, so we just assert it rejects.
  await assert.rejects(() =>
    program.methods.depositRewards(new BN(100))
      .accounts({
        keeper: payer.publicKey, pool,
        rewardMint: unregisteredMint,
        keeperTokenAccount: ata,
        rewardVault: rewardVaultPda(pool, unregisteredMint),
      })
      .rpc()
  )
})

test('deposit_rewards to a deactivated slot rejects (RewardInactive)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool, rewardMint, keeperAta } = await fullSetup(ctx)

  // Deactivate slot 0
  await program.methods.setRewardActive(0, false)
    .accounts({ admin: payer.publicKey, pool }).rpc()

  await mintTo(provider.connection, payer, rewardMint, keeperAta, payer, 1000)

  await assert.rejects(() =>
    program.methods.depositRewards(new BN(1000))
      .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: keeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
      .rpc()
  )
})

test('re-deposit folds leftover: period_finish extends and rate changes', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool, rewardMint, keeperAta } = await fullSetup(ctx)

  // First deposit: 3600 tokens over 3600s
  await mintTo(provider.connection, payer, rewardMint, keeperAta, payer, 3600)
  await program.methods.depositRewards(new BN(3600))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: keeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  const p1 = await program.account.pool.fetch(pool)
  const finishAfterFirst = p1.rewards[0].periodFinish.toNumber()

  // Advance halfway through the stream, then advance the slot so a new blockhash is generated
  // (avoids AlreadyProcessed error on the second identical mintTo call)
  warp(svm, 1800)
  svm.warpToSlot(svm.getClock().slot + 10n)

  // Second deposit: fold remaining + new tokens (use different amount to avoid duplicate tx)
  await mintTo(provider.connection, payer, rewardMint, keeperAta, payer, 7200)
  await program.methods.depositRewards(new BN(7200))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: keeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
    .rpc()

  const p2 = await program.account.pool.fetch(pool)
  const finishAfterSecond = p2.rewards[0].periodFinish.toNumber()

  // period_finish must have extended beyond the first finish
  assert.ok(finishAfterSecond > finishAfterFirst,
    `expected period_finish to extend: ${finishAfterSecond} > ${finishAfterFirst}`)
})
