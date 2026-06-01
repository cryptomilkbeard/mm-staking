import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, createMint, mintTo,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

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
