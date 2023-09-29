# RandomX PoW Wrapper

## Overview

The hardware capacity required to stand up the off-chain Fluence network is provided by a number of independent, high quality data centers including Filecoin Storage Providers (SP). en needed by serverless developers and their users, Fluence protocol incentivizes the commitment and provisioning of capacity to the network ahead of demand. The verification of such commitments is carried out by Fluence's Proof of Capacity (PoC). PoC is a set of solution components that in concert trigger the economic reward or slashing of providers.

Specifically, Fluence's Proof of Capacity is comprised of multiple components including:

* Proof of Work (PoW) component -- [RandomX](https://github.com/tevador/RandomX), off-chain
* PoW puzzle and difficulty component -- off-chain
* PoW proof generation component -- ZKP(s), off-chain
* Benchmarking framework -- off-chain
* EVM verifiers --  on-chain

[RandomX](https://github.com/tevador/RandomX) is the selected Proof of Work (PoW) system due to its production-grade maturity and GPU, FPGA and ASIC resistance. However, unlike the typical blockchain-based PoW, the Fluence use case does not follow the winner-takes-all (block) reward process. Instead, all participating capacity providers able to prove their capacity allocation, either through PoW or useful work (UW), are rewarded from treasury. Hence, special considerations to prevent replay attacks and capacity quality misrepresentation are in order.

Moreover, the Fluence protocol is first and foremost of utilizing provided capacity to carry out useful work (UW), i.e., the execution of paid serverless jobs, providers are enabled and encouraged to allocate capacity to either PoW or UW without losing out on incentives or having to re-stake. 

This wrapper code not only wraps RandomX PoW but handles the production of the K and N parameters to suit the Fluence protocol's needs as well as the dynamic deallocation and reallocation of capacity to PoW and UW. See Figure 1.

Figure 1: Stylized Execution Flow
```mermaid
    sequenceDiagram

    participant M as Main loop
    participant R as Randomx pool
    participant I as IO
    participant B as Blockchain

    M ->> I: init
    M ->> M: create crossbeam channel for puzzle solution communication
    M ->> B: Get blockheight and K
    M ->> M: update
    M ->> R: init RandomX instances
    par main
        loop main thread
            M ->> B: check K
            alt changed K
                M -> R: Restart all instances with new K
            end
            M ->> I: check for capacity realloc
            alt capacity realloc
                alt reduce alloc
                    M -> R: signal reduction in instances
                    M -> I: publish capacity unit (thread) names
                else increase alloc
                    M -> R: add more instances to pool
                end
            end
            M ->> M: check xbeam channel for solutions
            alt have golden hashes
                M ->> I: write each solution to a json file
            end
            M ->>  I: check for SIGTERM
            alt exit
                M ->> R: shutdown
                M ->> M: exit
            end
        end
    end
    par RandomX pool
        loop RandomX instance
            R ->> M: check for globals
            R ->> R: seed instance
            loop hasher
                R ->> R: create nonce
                R ->> R: create hash
                R ->> R: validate against puzzle
                alt golden hash
                    R ->> M: write K, H, GH raw and signed to xbeam channel
                end
                R ->> M: check globals
                alt reduce alloc
                    R ->> R: shutdown instance
                end
            end    
        end
    end
```

## Dependencies

The current implementation uses [rust-randomx](https://crates.io/crates/rust-randomx) 0.7.2 wrapper. Alternatively, [randomx-rs](https://crates.io/crates/randomx-rs) is also available with a higher download count and a more frequent update schedule. It might be worth benchmarking the `rust-randomx` and `randomx-rs` crates.

## Design & Implementation Considerations 

compute units, peers, ...

signing: peer_id
hashing: 

### Key

Key K generation follows the Monero template: a valid K is the most recent block divisible by 2048 and 64; that is, K changes roughly change 2.1 days (2048 blocks * 90 minutes per confirmed block) when pulling from the FVM mainet with another 1.5 hours delay (64 * 90), see [keyblock.rs]("./keyblock.rs"). Hence, the Randomx instances get re-iniitated/re-started every 2.1 days or so.

In order to verifiably tie K to a RandomX instance and server, via peer id, the "actual" is the signed (hash) of the eligible key block and thread id:

    K = Sign(Keccak(block_height, thread_id))

 where thread id =  Keccak(peer_id, idx) and currently mocked in lieu of on-chain generation. Of course, thread id can be easily adjusted to a core id or other compute unit definition. If K proof is part of a ZKP, the hashing of the inputs may be forgone in favor of a simple concatenated string as bytes for signing.


### Nonce

In a prototypical PoW scenario, N is the nonce of the hashed blob of the proposed block making it unique and easily verifiable. In the Fluence PoC context an alternative approach is required. While a pseudo-random number would do, a nonce with additional signals, such as a monotonically increasing nonce. For example, 

### Puzzle And Difficulty

The implementation utilizes the leading zeros puzzle. The associated difficulty needs to be benchmarked across server configurations and the desired expected golden hash period/epoch. It should be noted that a shorter epoch allows providers to be more responsive to switch from PoW to UW due the lower expected loss of not completing a hashing epoch.


### Capacity Reallocation

An integral aspect of PoC is a provider's ability to reallocate capacity between PoW and UW as smoothly and efficiently as possible. Moreover, a on-chain the capacity allocation to either PoW or UW is tracked on chain via a unique compute unit id that maps to the compute unit's stake. That is, PoW capacity reallocated to UW also triggers a move of associated stake from PoW to UW bucket. 

The current implementation trackes the max capacity available and adjust for reallocation specified in the [runtime config]("./data/runtime_cfg.json") file. Changing the dealloc value either up  decreases the number of Randomx instances or down increases the number of running Randomx instances. While compute unit ids are uses, i.e. [mocked]("./src/mcoks.rs") thread ids, and the reallocation reuqests are in numbers, a named reallocation is just easily accommodated. 

Note that currently the use of available capacity from reallocation is not covered. that is, Nox needs to be involved and allocate freed capacity to workers or vice versa.

## Optimization And Benchmarking Considerations

In order to prevent or at least massively limit the abuse of the capacity incentive program, this application needs to be optimized as much as possible and extensively benchmarked. See [benchmarking](https://www.notion.so/fluencenetwork/Proof-of-Work-Benchmarking-Pre-FLIP-9f1b8cdf6ab94ab2a6a77b31e33b02de?pvs=4) for more info.

## Nox Integration And Distribution Considerations

* At various discussion points there has been a desire to bind PoW to particles. For example, the json rpc call in [keyblock.rs]("./src/keyblock.rs") could be an Aqua call to a Marine service or Decider spell. 

* Bundling PoW with Nox looks reasonable and in line with operator expectations
  
* Nox needs to integrate with PoW for capacity management, i.e., allocated freed capacity to workers and vice versa.


## Operational Considerations

Since each thread makes a provider money, considerations have been given to minimize the thread overhead of "operational" thread overhead. Hence, a lot of even monitoring loops have been squeezed into the main thread.

### Thread Overhead

Needs to be kept (relatively) low. This not only requires optimized Fluence binaries but also a reasonable floor requirement on what consitutes a "server" in terms of cores and RAM. For example, we might want to consider a "Fluence server"  to be 64 or even 128 cores with 1 TB or 2TB RAM, respectively, to 


## Summary

To Dos:  

- [ ] need to settle on compute unit sizing which depends to some extent on optimal worker sizing. i.e., if a worker utilizes an even number of threads, a core-based unit is feasible otherwise thread--based model is more appropriate
- [ ] identify and define required Nox interfaces  




