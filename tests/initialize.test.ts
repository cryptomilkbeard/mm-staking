import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import { setup, poolPda, stakeVaultPda, createMint, BN, Keypair } from './helpers.ts'

test('initialize_pool sets fields and creates the stake vault', async () => {
  const { svm, provider, program, payer } = setup()
  const stakeMint = await createMint(provider.connection as any, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  const keeper = Keypair.generate().publicKey

  await program.methods
    .initializePool(new BN(3600), keeper)
    .accounts({
      admin: payer.publicKey,
      stakeMint,
      pool,
      stakeVault: stakeVaultPda(pool),
    })
    .rpc()

  const acc = await program.account.pool.fetch(pool)
  assert.equal(acc.admin.toBase58(), payer.publicKey.toBase58())
  assert.equal(acc.keeperAuthority.toBase58(), keeper.toBase58())
  assert.equal(acc.stakeMint.toBase58(), stakeMint.toBase58())
  assert.equal(acc.defaultDuration.toNumber(), 3600)
  assert.equal(acc.totalStaked.toNumber(), 0)
  assert.equal(acc.paused, 0)
})
