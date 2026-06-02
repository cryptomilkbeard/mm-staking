import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, stakerPda, createMint, mintTo, warp,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

async function initPool(ctx: any, keeperKey?: any) {
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  const keeper = keeperKey ?? Keypair.generate().publicKey
  await program.methods.initializePool(new BN(3600), keeper)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  return { stakeMint, pool, keeper }
}

test('admin setters update fields; non-admin rejected', async () => {
  const ctx = setup()
  const { program, provider, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), Keypair.generate().publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  await program.methods.setPaused(true).accounts({ admin: payer.publicKey, pool }).rpc()
  let p = await program.account.pool.fetch(pool)
  assert.equal(p.paused, 1)

  const newKeeper = Keypair.generate().publicKey
  await program.methods.setKeeperAuthority(newKeeper).accounts({ admin: payer.publicKey, pool }).rpc()
  p = await program.account.pool.fetch(pool)
  assert.equal(p.keeperAuthority.toBase58(), newKeeper.toBase58())

  // non-admin cannot pause
  const stranger = Keypair.generate()
  ;(ctx.svm as any).airdrop(stranger.publicKey, 1_000_000_000n)
  await assert.rejects(() => program.methods.setPaused(false)
    .accounts({ admin: stranger.publicKey, pool }).signers([stranger]).rpc())
})

test('set_paused blocks stake; allows unstake and emergency_withdraw', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPool(ctx)

  const userAta = getAssociatedTokenAddressSync(stakeMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, userAta, payer.publicKey, stakeMint)
  ), [payer])
  await mintTo(provider.connection, payer, stakeMint, userAta, payer, 1_000_000)

  // Stake before pause
  const staker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Pause
  await program.methods.setPaused(true).accounts({ admin: payer.publicKey, pool }).rpc()

  // Stake should be blocked
  await assert.rejects(() =>
    program.methods.stake(new BN(100_000))
      .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )

  // Unstake should still work
  await program.methods.unstake(new BN(200_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()
  const s = await program.account.stakerAccount.fetch(staker)
  assert.equal(s.stakedAmount.toNumber(), 300_000)

  // Emergency withdraw should also work
  await program.methods.emergencyWithdraw()
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()
  const s2 = await program.account.stakerAccount.fetch(staker)
  assert.equal(s2.stakedAmount.toNumber(), 0)
})

test('non-admin any setter rejects (Unauthorized)', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool } = await initPool(ctx)

  const stranger = Keypair.generate()
  ;(svm as any).airdrop(stranger.publicKey, 2_000_000_000n)

  // set_paused
  await assert.rejects(() =>
    program.methods.setPaused(true)
      .accounts({ admin: stranger.publicKey, pool })
      .signers([stranger]).rpc()
  )

  // set_keeper_authority
  await assert.rejects(() =>
    program.methods.setKeeperAuthority(Keypair.generate().publicKey)
      .accounts({ admin: stranger.publicKey, pool })
      .signers([stranger]).rpc()
  )

  // set_admin
  await assert.rejects(() =>
    program.methods.setAdmin(stranger.publicKey)
      .accounts({ admin: stranger.publicKey, pool })
      .signers([stranger]).rpc()
  )

  // set_duration (requires a registered reward slot, but the auth check fires first)
  await assert.rejects(() =>
    program.methods.setDuration(0, new BN(7200))
      .accounts({ admin: stranger.publicKey, pool })
      .signers([stranger]).rpc()
  )
})

test('set_keeper_authority: old keeper deposit rejected, new keeper deposit works', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool } = await initPool(ctx, payer.publicKey) // keeper = payer initially

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Create new keeper
  const newKeeper = Keypair.generate()
  ;(svm as any).airdrop(newKeeper.publicKey, 2_000_000_000n)
  await program.methods.setKeeperAuthority(newKeeper.publicKey)
    .accounts({ admin: payer.publicKey, pool }).rpc()

  // Old keeper (payer) deposit should now fail
  const payerAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, payerAta, payer.publicKey, rewardMint)
  ), [payer])
  await mintTo(provider.connection, payer, rewardMint, payerAta, payer, 1000)

  await assert.rejects(() =>
    program.methods.depositRewards(new BN(100))
      .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: payerAta, rewardVault: rewardVaultPda(pool, rewardMint) })
      .rpc()
  )

  // New keeper deposit should succeed
  const newKeeperAta = getAssociatedTokenAddressSync(rewardMint, newKeeper.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(newKeeper.publicKey, newKeeperAta, newKeeper.publicKey, rewardMint)
  ), [newKeeper])
  await mintTo(provider.connection, payer, rewardMint, newKeeperAta, payer, 500)

  await program.methods.depositRewards(new BN(500))
    .accounts({ keeper: newKeeper.publicKey, pool, rewardMint, keeperTokenAccount: newKeeperAta, rewardVault: rewardVaultPda(pool, rewardMint) })
    .signers([newKeeper])
    .rpc()

  const p = await program.account.pool.fetch(pool)
  assert.ok(p.rewards[0].periodFinish.toNumber() > 0)
})

test('set_admin: old admin rejected, new admin works', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool } = await initPool(ctx)

  const newAdmin = Keypair.generate()
  ;(svm as any).airdrop(newAdmin.publicKey, 2_000_000_000n)

  // Transfer admin to newAdmin
  await program.methods.setAdmin(newAdmin.publicKey)
    .accounts({ admin: payer.publicKey, pool }).rpc()

  // Old admin (payer) must now be rejected
  await assert.rejects(() =>
    program.methods.setPaused(true)
      .accounts({ admin: payer.publicKey, pool }).rpc()
  )

  // New admin can update
  await program.methods.setPaused(true)
    .accounts({ admin: newAdmin.publicKey, pool })
    .signers([newAdmin])
    .rpc()

  const p = await program.account.pool.fetch(pool)
  assert.equal(p.paused, 1)
  assert.equal(p.admin.toBase58(), newAdmin.publicKey.toBase58())
})

test('set_duration happy path updates the reward slot duration', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool } = await initPool(ctx)

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(3600))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  await program.methods.setDuration(0, new BN(7200))
    .accounts({ admin: payer.publicKey, pool }).rpc()

  const p = await program.account.pool.fetch(pool)
  assert.equal(p.rewards[0].duration.toNumber(), 7200)
})

test('set_duration with duration <= 0 rejects (InvalidDuration)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool } = await initPool(ctx)

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(0))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  await assert.rejects(() =>
    program.methods.setDuration(0, new BN(0))
      .accounts({ admin: payer.publicKey, pool }).rpc()
  )

  await assert.rejects(() =>
    program.methods.setDuration(0, new BN(-1))
      .accounts({ admin: payer.publicKey, pool }).rpc()
  )
})

test('set_duration on empty/unregistered slot rejects (RewardNotFound)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool } = await initPool(ctx)
  // No rewards registered

  await assert.rejects(() =>
    program.methods.setDuration(0, new BN(7200))
      .accounts({ admin: payer.publicKey, pool }).rpc()
  )
})
