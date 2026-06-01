import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import { setup, poolPda, stakeVaultPda, createMint, BN, Keypair } from './helpers.ts'

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
