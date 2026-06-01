import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, stakerPda, createMint, mintTo,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction,
  BN, Keypair, PublicKey,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

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
