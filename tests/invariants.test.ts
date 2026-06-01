import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, stakerPda, createMint, mintTo, getAccount,
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
