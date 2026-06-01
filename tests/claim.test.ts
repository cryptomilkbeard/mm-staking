import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, stakerPda, createMint, mintTo, getAccount,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, TOKEN_PROGRAM_ID, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

test('single staker claims ~all streamed rewards after the period', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(100), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const rewardMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  await program.methods.addReward(new BN(100))
    .accounts({ admin: payer.publicKey, pool, rewardMint, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // user stakes
  const mmAta = getAssociatedTokenAddressSync(stakeMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, mmAta, payer.publicKey, stakeMint)), [payer])
  await mintTo(provider.connection, payer, stakeMint, mmAta, payer, 1_000_000)
  const staker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(1_000_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: mmAta, stakeVault: stakeVaultPda(pool) }).rpc()

  // keeper deposits 1000 reward over 100s
  const kAta = getAssociatedTokenAddressSync(rewardMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rewardMint)), [payer])
  await mintTo(provider.connection, payer, rewardMint, kAta, payer, 1000)
  await program.methods.depositRewards(new BN(1000))
    .accounts({ keeper: payer.publicKey, pool, rewardMint, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rewardMint) }).rpc()

  // advance the clock past the period
  const clock = svm.getClock()
  clock.unixTimestamp = clock.unixTimestamp + 200n
  svm.setClock(clock)

  // claim with remaining_accounts = [rewardVault, userRewardAta]
  await program.methods.claim()
    .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rewardMint), isSigner: false, isWritable: true },
      { pubkey: kAta, isSigner: false, isWritable: true },
    ])
    .rpc()

  const bal = await getAccount(provider.connection, kAta)
  // sole staker over a finished period gets ~all 1000 (allow tiny rounding dust to the vault)
  assert.ok(Number(bal.amount) >= 999 && Number(bal.amount) <= 1000, `got ${bal.amount}`)
})
