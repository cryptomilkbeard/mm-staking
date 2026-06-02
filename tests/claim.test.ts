import { test } from 'node:test'
import { strict as assert } from 'node:assert'
import {
  setup, poolPda, stakeVaultPda, rewardVaultPda, stakerPda, createMint, mintTo, getAccount, warp,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction, TOKEN_PROGRAM_ID, BN, Keypair,
} from './helpers.ts'
import { Transaction } from '@solana/web3.js'

/** Full setup: pool + one or more reward mints + staker with stake */
async function buildClaimCtx(ctx: any, opts: { stakeAmount?: number; rewardMints?: number; depositPerMint?: number; streamSeconds?: number } = {}) {
  const {
    stakeAmount = 1_000_000,
    rewardMints = 1,
    depositPerMint = 1000,
    streamSeconds = 100,
  } = opts
  const { svm, provider, program, payer } = ctx

  const stakeMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const pool = poolPda(stakeMint)
  await program.methods.initializePool(new BN(streamSeconds), payer.publicKey)
    .accounts({ admin: payer.publicKey, stakeMint, pool, stakeVault: stakeVaultPda(pool) }).rpc()

  // Add N reward mints
  const rewardMintList: any[] = []
  for (let i = 0; i < rewardMints; i++) {
    const rm = await createMint(provider.connection, payer, payer.publicKey, null, 6)
    await program.methods.addReward(new BN(streamSeconds))
      .accounts({ admin: payer.publicKey, pool, rewardMint: rm, rewardVault: rewardVaultPda(pool, rm) }).rpc()
    rewardMintList.push(rm)
  }

  // User stakes
  const mmAta = getAssociatedTokenAddressSync(stakeMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, mmAta, payer.publicKey, stakeMint)
  ), [payer])
  await mintTo(provider.connection, payer, stakeMint, mmAta, payer, stakeAmount)
  const staker = stakerPda(pool, payer.publicKey)
  await program.methods.stake(new BN(stakeAmount))
    .accounts({ owner: payer.publicKey, pool, staker, stakeMint, userTokenAccount: mmAta, stakeVault: stakeVaultPda(pool) })
    .rpc()

  // Keeper deposits into each reward vault
  for (const rm of rewardMintList) {
    const kAta = getAssociatedTokenAddressSync(rm, payer.publicKey)
    await provider.sendAndConfirm(new Transaction().add(
      createAssociatedTokenAccountInstruction(payer.publicKey, kAta, payer.publicKey, rm)
    ), [payer])
    await mintTo(provider.connection, payer, rm, kAta, payer, depositPerMint)
    await program.methods.depositRewards(new BN(depositPerMint))
      .accounts({ keeper: payer.publicKey, pool, rewardMint: rm, keeperTokenAccount: kAta, rewardVault: rewardVaultPda(pool, rm) })
      .rpc()
  }

  return { pool, stakeMint, staker, mmAta, rewardMintList }
}

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

test('claim with multiple rewards in one tx (multi-vault remaining_accounts)', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool, staker, rewardMintList } = await buildClaimCtx(ctx, {
    rewardMints: 3,
    depositPerMint: 1000,
    streamSeconds: 100,
  })

  // Advance past the period
  warp(svm, 200)

  // Build remaining_accounts with all 3 reward pairs
  const remainingAccounts: any[] = []
  const userAtas: any[] = []
  for (const rm of rewardMintList) {
    const userAta = getAssociatedTokenAddressSync(rm, payer.publicKey)
    // userAta is already created (keeper ata used as claim target too — same pubkey for payer)
    remainingAccounts.push({ pubkey: rewardVaultPda(pool, rm), isSigner: false, isWritable: true })
    remainingAccounts.push({ pubkey: userAta, isSigner: false, isWritable: true })
    userAtas.push(userAta)
  }

  await program.methods.claim()
    .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts(remainingAccounts)
    .rpc()

  // Each of the 3 reward ATAs should have received ~1000
  for (let i = 0; i < 3; i++) {
    const bal = await getAccount(provider.connection, userAtas[i])
    assert.ok(Number(bal.amount) >= 999 && Number(bal.amount) <= 1000, `reward[${i}] got ${bal.amount}`)
  }
})

test('claim with nothing accrued is a no-op (0 received, succeeds)', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool, staker, rewardMintList } = await buildClaimCtx(ctx, {
    rewardMints: 1,
    depositPerMint: 1000,
    streamSeconds: 100,
  })

  const rm = rewardMintList[0]
  const userAta = getAssociatedTokenAddressSync(rm, payer.publicKey)

  // Do NOT advance time — nothing has accrued yet (or very nearly nothing)
  // Claim immediately — should succeed but transfer 0 (or near-0)
  const balBefore = await getAccount(provider.connection, userAta)

  await program.methods.claim()
    .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
    .remainingAccounts([
      { pubkey: rewardVaultPda(pool, rm), isSigner: false, isWritable: true },
      { pubkey: userAta, isSigner: false, isWritable: true },
    ])
    .rpc()

  const balAfter = await getAccount(provider.connection, userAta)
  // At time=0 virtually nothing should have accrued — the front-running invariant test already
  // verifies this. Here we just assert the instruction succeeded (no throw).
  assert.ok(Number(balAfter.amount) <= Number(balBefore.amount) + 5, 'expected near-zero claim')
})

test('claim with odd-length remaining_accounts rejects (VaultMismatch)', async () => {
  const ctx = setup()
  const { provider, program, payer } = ctx
  const { pool, staker, rewardMintList } = await buildClaimCtx(ctx, { rewardMints: 1 })

  const rm = rewardMintList[0]

  // Provide only 1 account (odd) instead of a vault+ata pair
  await assert.rejects(() =>
    program.methods.claim()
      .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
      .remainingAccounts([
        { pubkey: rewardVaultPda(pool, rm), isSigner: false, isWritable: true },
      ])
      .rpc()
  )
})

test('claim with wrong reward_vault in remaining accounts rejects (VaultMismatch)', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool, staker, rewardMintList } = await buildClaimCtx(ctx, { rewardMints: 1, depositPerMint: 1000 })

  // Advance so there's something to claim
  warp(svm, 200)

  const rm = rewardMintList[0]
  const userAta = getAssociatedTokenAddressSync(rm, payer.publicKey)

  // Use an entirely wrong pubkey as vault (stake vault, not reward vault)
  const wrongVault = stakeVaultPda(pool)

  await assert.rejects(() =>
    program.methods.claim()
      .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
      .remainingAccounts([
        { pubkey: wrongVault, isSigner: false, isWritable: true },
        { pubkey: userAta, isSigner: false, isWritable: true },
      ])
      .rpc()
  )
})

test('claim with user ATA of the wrong mint rejects (MintMismatch)', async () => {
  const ctx = setup()
  const { svm, provider, program, payer } = ctx
  const { pool, staker, rewardMintList, stakeMint } = await buildClaimCtx(ctx, {
    rewardMints: 1,
    depositPerMint: 1000,
    streamSeconds: 100,
  }) as any

  warp(svm, 200)

  const rm = rewardMintList[0]

  // Create a fresh ATA for the stake mint (wrong mint) to pass as the user reward ATA
  // We need the stake mint ATA to already exist. The user has a stake ATA already — let's
  // create a separate mint and ATA as the "wrong" one.
  const wrongMint = await createMint(provider.connection, payer, payer.publicKey, null, 6)
  const wrongAta = getAssociatedTokenAddressSync(wrongMint, payer.publicKey)
  await provider.sendAndConfirm(new Transaction().add(
    createAssociatedTokenAccountInstruction(payer.publicKey, wrongAta, payer.publicKey, wrongMint)
  ), [payer])

  await assert.rejects(() =>
    program.methods.claim()
      .accounts({ owner: payer.publicKey, pool, staker, tokenProgram: TOKEN_PROGRAM_ID })
      .remainingAccounts([
        { pubkey: rewardVaultPda(pool, rm), isSigner: false, isWritable: true },
        { pubkey: wrongAta, isSigner: false, isWritable: true },
      ])
      .rpc()
  )
})
