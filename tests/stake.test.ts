import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, stakerPda, createMint, mintTo,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction,
  BN, Keypair, PublicKey,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

async function initPoolAndStakeMint(ctx: any) {
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), Keypair.generate().publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  return { stakeMint, pool }
}

async function setupUserAta(ctx: any, stakeMint: PublicKey, user: any, amount: number) {
  const { provider, program, payer } = ctx
  const ata = getAssociatedTokenAddressSync(stakeMint, user.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(user.publicKey, ata, user.publicKey, stakeMint)
  ), [user])
  await mintTo(provider.connection, payer, stakeMint, ata, payer, amount)
  return ata
}

test('stake then unstake moves MM and updates totals', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), Keypair.generate().publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const userAta = getAssociatedTokenAddressSync(stakeMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, userAta, payer.publicKey, stakeMint)
  ), [payer])
  await mintTo(provider.connection, payer, stakeMint, userAta, payer, 1_000_000)

  const staker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(400_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  let p = await program.account.pool.fetch(pool)
  let s = await program.account.stakerAccount.fetch(staker)
  assert.equal(p.totalStaked.toNumber(), 400_000)
  assert.equal(s.stakedAmount.toNumber(), 400_000)

  await program.methods.unstake(new BN(150_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  p = await program.account.pool.fetch(pool)
  s = await program.account.stakerAccount.fetch(staker)
  assert.equal(p.totalStaked.toNumber(), 250_000)
  assert.equal(s.stakedAmount.toNumber(), 250_000)
})

test('stake with amount == 0 rejects (ZeroAmount)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  await assert.rejects(() =>
    program.methods.stake(new BN(0))
      .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})

test('stake when pool is paused rejects (Paused)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  // Pause the pool
  await program.methods.setPaused(true).accounts({ admin: payer.publicKey, pool }).rpc()

  await assert.rejects(() =>
    program.methods.stake(new BN(500_000))
      .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})

test('re-stake from same wallet accrues and adds to existing stake', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  // First stake
  await program.methods.stake(new BN(300_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Second stake — should add, not replace
  await program.methods.stake(new BN(200_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  const s = await program.account.stakerAccount.fetch(staker)
  const p = await program.account.pool.fetch(pool)
  assert.equal(s.stakedAmount.toNumber(), 500_000)
  assert.equal(p.totalStaked.toNumber(), 500_000)
})

test('stake from a second wallet creates a second independent staker PDA', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)

  const alice = payer
  const bob = Keypair.generate()
  ;(svm as any).airdrop(bob.publicKey, 2_000_000_000n)

  const aliceAta = await setupUserAta(ctx, stakeMint, alice, 1_000_000)
  const bobAta = await setupUserAta(ctx, stakeMint, bob, 1_000_000)

  const aliceStaker = stakerPda(pool, alice.publicKey)
  const bobStaker = stakerPda(pool, bob.publicKey)

  await program.methods.stake(new BN(400_000))
    .accounts({ owner: alice.publicKey, pool, staker: aliceStaker, stakeMint, userTokenAccount: aliceAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  await program.methods.stake(new BN(600_000))
    .accounts({ owner: bob.publicKey, pool, staker: bobStaker, stakeMint, userTokenAccount: bobAta, stakeVault: stakeVaultPda(pool) })
    .signers([bob])
    .rpc()

  const sa = await program.account.stakerAccount.fetch(aliceStaker)
  const sb = await program.account.stakerAccount.fetch(bobStaker)
  const p = await program.account.pool.fetch(pool)

  assert.equal(sa.stakedAmount.toNumber(), 400_000)
  assert.equal(sb.stakedAmount.toNumber(), 600_000)
  assert.equal(p.totalStaked.toNumber(), 1_000_000)
  // PDAs are distinct accounts
  assert.notEqual(aliceStaker.toBase58(), bobStaker.toBase58())
})

// --- unstake error paths ---

test('unstake with amount == 0 rejects (ZeroAmount)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  await program.methods.stake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  await assert.rejects(() =>
    program.methods.unstake(new BN(0))
      .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})

test('unstake more than staked rejects (InsufficientStake)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  await program.methods.stake(new BN(300_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  await assert.rejects(() =>
    program.methods.unstake(new BN(999_999))
      .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})

test('unstake works even when pool is paused (principal stays exitable)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)
  const userAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)

  await program.methods.stake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Pause
  await program.methods.setPaused(true).accounts({ admin: payer.publicKey, pool }).rpc()

  // Unstake should still work
  await program.methods.unstake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: userAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  const s = await program.account.stakerAccount.fetch(staker)
  assert.equal(s.stakedAmount.toNumber(), 0)
})

test('unstake with wrong-owner staker PDA rejects', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { stakeMint, pool } = await initPoolAndStakeMint(ctx)

  // Create staker for payer
  const payerAta = await setupUserAta(ctx, stakeMint, payer, 1_000_000)
  const payerStaker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker: payerStaker, stakeMint, userTokenAccount: payerAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Bob tries to unstake using payer's staker PDA — must fail (has_one = owner)
  const bob = Keypair.generate()
  ;(svm as any).airdrop(bob.publicKey, 2_000_000_000n)
  const bobAta = await setupUserAta(ctx, stakeMint, bob, 0)

  await assert.rejects(() =>
    program.methods.unstake(new BN(100_000))
      .accounts({ owner: bob.publicKey, pool, staker: payerStaker, stakeMint, userTokenAccount: bobAta, stakeVault: stakeVaultPda(pool) })
      .signers([bob])
      .rpc()
  )
})
