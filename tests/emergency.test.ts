import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, stakerPda, createMint, mintTo, getAccount,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

test('emergency_withdraw returns principal even when paused', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(3600), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  const mmAta = getAssociatedTokenAddressSync(stakeMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, mmAta, payer.publicKey, stakeMint)), [payer])
  await mintTo(provider.connection, payer, stakeMint, mmAta, payer, 500_000)
  const staker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(500_000))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: mmAta, stakeVault: stakeVaultPda(pool) }).rpc()

  // pause the pool
  await program.methods.setPaused(true).accounts({ admin: payer.publicKey, pool }).rpc()

  await program.methods.emergencyWithdraw()
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: mmAta, stakeVault: stakeVaultPda(pool) }).rpc()

  const s = await program.account.stakerAccount.fetch(staker)
  const bal = await getAccount(provider.connection, mmAta)
  assert.equal(s.stakedAmount.toNumber(), 0)
  assert.equal(Number(bal.amount), 500_000)
})
