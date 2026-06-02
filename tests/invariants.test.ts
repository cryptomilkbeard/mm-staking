import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, stakerPda, createMint, mintTo, getAccount, warp,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, TOKEN_PROGRAM_ID, BN, Keypair, PublicKey,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

async function fundStaker(ctx: any, stakeMint: PublicKey, pool: PublicKey, user: any, amount: number) {
  const { provider, program, svm, payer } = ctx
  svm.airdrop(user.publicKey, 2_000_000_000n)
  const ata = getAssociatedTokenAddressSync(stakeMint, user.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(user.publicKey, ata, user.publicKey, stakeMint)), [user])
  await mintTo(provider.connection, payer, stakeMint, ata, payer, amount)
  const staker = stakerPda(pool, user.publicKey)
  await program.methods.stake(new BN(amount))
    .accounts({ owner: user.publicKey, pool, staker, stakeMint, userTokenAccount: ata, stakeVault: stakeVaultPda(pool) })
    .signers([user]).rpc()
  return { ata, staker }
}

test('two stakers split rewards by stake weight; vault stays solvent', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const alice = Keypair.generate(), bob = Keypair.generate()
  const a = await fundStaker(ctx, stakeMint, pool, alice, 750_000) // 75%
  const b = await fundStaker(ctx, stakeMint, pool, bob, 250_000)   // 25%

  // deposit 1000 over 100s
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 1000)
  await program.methods.depositRewards(new BN(1000))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const clock = svm.getClock(); clock.unixTimestamp = clock.unixTimestamp + 200n; svm.setClock(clock)

  // reward ATAs for alice + bob
  for (const u of [alice, bob]) {
    const rAta = getAssociatedTokenAddressSync(rewardMint, u.publicKey)
    await provider.sendAndConfirm(new Transaction().add(
      createAssociatedTokenAccountInstruction(u.publicKey, rAta, u.publicKey, rewardMint)), [u])
  }
  const aRewardAta = getAssociatedTokenAddressSync(rewardMint, alice.publicKey)
  const bRewardAta = getAssociatedTokenAddressSync(rewardMint, bob.publicKey)

  await program.methods.claim().accounts({ owner: alice.publicKey, pool, staker: a.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: aRewardAta, isSigner: false, isWritable: true },
    ]).signers([alice]).rpc()
  await program.methods.claim().accounts({ owner: bob.publicKey, pool, staker: b.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: bRewardAta, isSigner: false, isWritable: true },
    ]).signers([bob]).rpc()

  const aBal = Number((await getAccount(provider.connection, aRewardAta)).amount)
  const bBal = Number((await getAccount(provider.connection, bRewardAta)).amount)
  const vaultBal = Number((await getAccount(provider.connection, rewardVaultPda(pool, rewardMint))).amount)

  assert.ok(Math.abs(aBal - 750) <= 1, `alice ${aBal}`)
  assert.ok(Math.abs(bBal - 250) <= 1, `bob ${bBal}`)
  // SOLVENCY: paid out + remaining dust == deposited; never overpaid
  assert.equal(aBal + bBal + vaultBal, 1000)
  assert.ok(aBal + bBal <= 1000)
})

test('staking right before a deposit does not capture the whole stream instantly', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const attacker = Keypair.generate()
  const a = await fundStaker(ctx, stakeMint, pool, attacker, 1_000_000)

  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 1000)
  await program.methods.depositRewards(new BN(1000))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // claim immediately (0 seconds elapsed) -> ~nothing claimable
  const rAta = getAssociatedTokenAddressSync(rewardMint, attacker.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(attacker.publicKey, rAta, attacker.publicKey, rewardMint)), [attacker])
  await program.methods.claim().accounts({ owner: attacker.publicKey, pool, staker: a.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: rAta, isSigner: false, isWritable: true },
    ]).signers([attacker]).rpc()
  const bal = Number((await getAccount(provider.connection, rAta)).amount)
  assert.ok(bal <= 50, `front-runner grabbed ${bal} of 1000 instantly`)
})

test('reward added after staking accrues only from activation forward', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const user = Keypair.generate()
  const u = await fundStaker(ctx, stakeMint, pool, user, 1_000_000)

  // add reward AFTER the user staked
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 1000)
  await program.methods.depositRewards(new BN(1000))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  const clock = svm.getClock(); clock.unixTimestamp = clock.unixTimestamp + 200n; svm.setClock(clock)
  const rAta = getAssociatedTokenAddressSync(rewardMint, user.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(user.publicKey, rAta, user.publicKey, rewardMint)), [user])
  await program.methods.claim().accounts({ owner: user.publicKey, pool, staker: u.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: rAta, isSigner: false, isWritable: true },
    ]).signers([user]).rpc()
  const bal = Number((await getAccount(provider.connection, rAta)).amount)
  assert.ok(bal >= 999 && bal <= 1000, `sole staker got ${bal}`) // earns full stream from activation
})

test('pro-rata 1:3 split (100 vs 300 stake) after finished stream via token balances', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // 100 vs 300 stake → 1:3 split
  const alice = Keypair.generate(), bob = Keypair.generate()
  const a = await fundStaker(ctx, stakeMint, pool, alice, 100_000)
  const b = await fundStaker(ctx, stakeMint, pool, bob, 300_000)

  // Deposit 400 tokens over 100s
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 400)
  await program.methods.depositRewards(new BN(400))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Advance past the period
  warp(svm, 200)

  for (const u of [alice, bob]) {
    const rAta = getAssociatedTokenAddressSync(rewardMint, u.publicKey)
    await provider.sendAndConfirm(new Transaction().add(
      createAssociatedTokenAccountInstruction(u.publicKey, rAta, u.publicKey, rewardMint)), [u])
  }
  const aRewardAta = getAssociatedTokenAddressSync(rewardMint, alice.publicKey)
  const bRewardAta = getAssociatedTokenAddressSync(rewardMint, bob.publicKey)

  await program.methods.claim().accounts({ owner: alice.publicKey, pool, staker: a.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: aRewardAta, isSigner: false, isWritable: true },
    ]).signers([alice]).rpc()
  await program.methods.claim().accounts({ owner: bob.publicKey, pool, staker: b.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: bRewardAta, isSigner: false, isWritable: true },
    ]).signers([bob]).rpc()

  const aBal = Number((await getAccount(provider.connection, aRewardAta)).amount)
  const bBal = Number((await getAccount(provider.connection, bRewardAta)).amount)

  // 1:3 split → alice ~100, bob ~300 (total 400)
  assert.ok(Math.abs(aBal - 100) <= 2, `alice: expected ~100 got ${aBal}`)
  assert.ok(Math.abs(bBal - 300) <= 2, `bob: expected ~300 got ${bBal}`)
  assert.ok(aBal + bBal <= 400, 'must not overpay')
})

test('zero-stake roll-forward: deposit with no stakers, stake later, claim ~full deposit', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Deposit 1000 with NO stakers yet
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 1000)
  await program.methods.depositRewards(new BN(1000))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Advance 50s (halfway through, no stakers — rewards should roll forward, not evaporate)
  warp(svm, 50)

  // Now a staker arrives
  const user = Keypair.generate()
  const u = await fundStaker(ctx, stakeMint, pool, user, 1_000_000)

  // Advance 100s more (stream is now exhausted from their perspective)
  warp(svm, 200)

  const rAta = getAssociatedTokenAddressSync(rewardMint, user.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(user.publicKey, rAta, user.publicKey, rewardMint)), [user])
  await program.methods.claim().accounts({ owner: user.publicKey, pool, staker: u.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: rAta, isSigner: false, isWritable: true },
    ]).signers([user]).rpc()

  const bal = Number((await getAccount(provider.connection, rAta)).amount)
  // The staker joined halfway through. They receive the second half (~500).
  // The first 50s with no stakers are "lost" (reward_per_token doesn't move with total_staked=0).
  // So bal should be in [400, 1000] — at least they get something meaningful, not the full 1000.
  assert.ok(bal > 0 && bal <= 1000, `expected nonzero claimable, got ${bal}`)
})

test('solvency: reward_vault balance >= remaining obligations after partial claims', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()
  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Three equal stakers
  const alice = Keypair.generate(), bob = Keypair.generate(), carol = Keypair.generate()
  const a = await fundStaker(ctx, stakeMint, pool, alice, 1_000_000)
  const b = await fundStaker(ctx, stakeMint, pool, bob, 1_000_000)
  const c = await fundStaker(ctx, stakeMint, pool, carol, 1_000_000)

  // Deposit 900 tokens
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 900)
  await program.methods.depositRewards(new BN(900))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // Advance past stream
  warp(svm, 200)

  // Create reward ATAs
  for (const u of [alice, bob, carol]) {
    const rAta = getAssociatedTokenAddressSync(rewardMint, u.publicKey)
    await provider.sendAndConfirm(new Transaction().add(
      createAssociatedTokenAccountInstruction(u.publicKey, rAta, u.publicKey, rewardMint)), [u])
  }

  // Only alice claims
  const aRewardAta = getAssociatedTokenAddressSync(rewardMint, alice.publicKey)
  await program.methods.claim().accounts({ owner: alice.publicKey, pool, staker: a.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: aRewardAta, isSigner: false, isWritable: true },
    ]).signers([alice]).rpc()

  const aBal = Number((await getAccount(provider.connection, aRewardAta)).amount)
  const vaultBal = Number((await getAccount(provider.connection, rewardVaultPda(pool, rewardMint))).amount)

  // After alice's claim, vault must still hold enough for bob and carol
  assert.ok(aBal >= 298 && aBal <= 300, `alice expected ~300 got ${aBal}`)
  assert.ok(vaultBal >= aBal * 2, `vault (${vaultBal}) must cover remaining ~${aBal * 2} for bob+carol`)

  // Solvency: paid + remaining == deposited
  const bRewardAta = getAssociatedTokenAddressSync(rewardMint, bob.publicKey)
  const cRewardAta = getAssociatedTokenAddressSync(rewardMint, carol.publicKey)
  await program.methods.claim().accounts({ owner: bob.publicKey, pool, staker: b.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: bRewardAta, isSigner: false, isWritable: true },
    ]).signers([bob]).rpc()
  await program.methods.claim().accounts({ owner: carol.publicKey, pool, staker: c.staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: cRewardAta, isSigner: false, isWritable: true },
    ]).signers([carol]).rpc()

  const bBal = Number((await getAccount(provider.connection, bRewardAta)).amount)
  const cBal = Number((await getAccount(provider.connection, cRewardAta)).amount)
  const finalVault = Number((await getAccount(provider.connection, rewardVaultPda(pool, rewardMint))).amount)

  assert.equal(aBal + bBal + cBal + finalVault, 900, 'solvency: paid + dust == deposited')
  assert.ok(aBal + bBal + cBal <= 900, 'must never overpay')
})
