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

test('initialize_pool rejects default_duration <= 0 (InvalidDuration)', async () => {
  const { provider, program, payer } = setup()
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)

  let threw = false
  try {
    await program.methods
      .initializePool(new BN(0), Keypair.generate().publicKey)
      .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) })
      .rpc()
  } catch (e: any) {
    threw = true
    assert.ok(e.message.includes('InvalidDuration') || e.message.includes('6009') || e.logs?.some((l: string) => l.includes('InvalidDuration')),
      `expected InvalidDuration, got: ${e.message}`)
  }
  assert.ok(threw, 'expected instruction to reject')
})

test('initialize_pool rejects negative default_duration (InvalidDuration)', async () => {
  const { provider, program, payer } = setup()
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)

  await assert.rejects(() =>
    program.methods
      .initializePool(new BN(-1), Keypair.generate().publicKey)
      .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})

test('initialize_pool rejects re-initialization of the same pool (account already in use)', async () => {
  const { provider, program, payer } = setup()
  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  const keeper = Keypair.generate().publicKey

  // First init succeeds
  await program.methods
    .initializePool(new BN(3600), keeper)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Second init with the same mint must fail
  await assert.rejects(() =>
    program.methods
      .initializePool(new BN(7200), keeper)
      .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) })
      .rpc()
  )
})
