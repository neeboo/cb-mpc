use cb_mpc::{
    dkg, refresh, sign, EcdsaKeyShare, SessionId, ThresholdPolicy, Transport, TransportError,
};
use k256::ecdsa::signature::hazmat::PrehashVerifier;
use k256::ecdsa::{Signature, VerifyingKey};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const TRANSPORT_FAILURE: i32 = 0xff03_0001_u32 as i32;

struct Queue {
    messages: Mutex<VecDeque<Vec<u8>>>,
    available: Condvar,
}

impl Queue {
    fn new() -> Self {
        Self {
            messages: Mutex::new(VecDeque::new()),
            available: Condvar::new(),
        }
    }

    fn push(&self, message: Vec<u8>) {
        self.messages.lock().unwrap().push_back(message);
        self.available.notify_one();
    }

    fn pop(&self) -> Result<Vec<u8>, TransportError> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut messages = self.messages.lock().unwrap();
        loop {
            if let Some(message) = messages.pop_front() {
                return Ok(message);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(TransportError::new(TRANSPORT_FAILURE));
            }
            let (next, wait) = self
                .available
                .wait_timeout(messages, deadline - now)
                .unwrap();
            messages = next;
            if wait.timed_out() && messages.is_empty() {
                return Err(TransportError::new(TRANSPORT_FAILURE));
            }
        }
    }
}

struct Network {
    // queues[receiver][sender]
    queues: Vec<Vec<Queue>>,
    sends: Vec<AtomicUsize>,
    receives: Vec<AtomicUsize>,
    receive_all_calls: Vec<AtomicUsize>,
}

impl Network {
    fn new(party_count: usize) -> Arc<Self> {
        Arc::new(Self {
            queues: (0..party_count)
                .map(|_| (0..party_count).map(|_| Queue::new()).collect())
                .collect(),
            sends: (0..party_count).map(|_| AtomicUsize::new(0)).collect(),
            receives: (0..party_count).map(|_| AtomicUsize::new(0)).collect(),
            receive_all_calls: (0..party_count).map(|_| AtomicUsize::new(0)).collect(),
        })
    }

    fn transport(self: &Arc<Self>, self_index: usize) -> LocalTransport {
        LocalTransport {
            self_index,
            network: Arc::clone(self),
        }
    }

    fn stats(&self, elapsed: Duration) -> PhaseStats {
        PhaseStats {
            elapsed,
            sends: self
                .sends
                .iter()
                .map(|count| count.load(Ordering::Relaxed))
                .collect(),
            receives: self
                .receives
                .iter()
                .map(|count| count.load(Ordering::Relaxed))
                .collect(),
            receive_all_calls: self
                .receive_all_calls
                .iter()
                .map(|count| count.load(Ordering::Relaxed))
                .collect(),
        }
    }
}

#[derive(Debug)]
struct PhaseStats {
    elapsed: Duration,
    sends: Vec<usize>,
    receives: Vec<usize>,
    receive_all_calls: Vec<usize>,
}

impl PhaseStats {
    fn log(&self, phase: &str) {
        eprintln!(
            "{phase}_STATS elapsed_ns={} sends={:?} receives={:?} receive_all_calls={:?}",
            self.elapsed.as_nanos(),
            self.sends,
            self.receives,
            self.receive_all_calls
        );
    }
}

struct LocalTransport {
    self_index: usize,
    network: Arc<Network>,
}

impl Transport for LocalTransport {
    fn send(&self, receiver: i32, data: &[u8]) -> Result<(), TransportError> {
        let receiver = usize::try_from(receiver)
            .ok()
            .filter(|index| *index < self.network.queues.len())
            .ok_or_else(|| TransportError::new(TRANSPORT_FAILURE))?;
        self.network.sends[self.self_index].fetch_add(1, Ordering::Relaxed);
        self.network.queues[receiver][self.self_index].push(data.to_vec());
        Ok(())
    }

    fn receive(&self, sender: i32) -> Result<Vec<u8>, TransportError> {
        let sender = usize::try_from(sender)
            .ok()
            .filter(|index| *index < self.network.queues.len())
            .ok_or_else(|| TransportError::new(TRANSPORT_FAILURE))?;
        self.network.receives[self.self_index].fetch_add(1, Ordering::Relaxed);
        self.network.queues[self.self_index][sender].pop()
    }

    fn receive_all(&self, senders: &[i32]) -> Result<Vec<Vec<u8>>, TransportError> {
        self.network.receive_all_calls[self.self_index].fetch_add(1, Ordering::Relaxed);
        senders.iter().map(|sender| self.receive(*sender)).collect()
    }
}

fn run_parties<T: Send>(
    party_count: usize,
    operation: impl Fn(usize, &LocalTransport) -> T + Sync,
) -> (Vec<T>, PhaseStats) {
    let network = Network::new(party_count);
    let started = Instant::now();
    let results = std::thread::scope(|scope| {
        let mut threads = Vec::with_capacity(party_count);
        for index in 0..party_count {
            let transport = network.transport(index);
            let operation = &operation;
            threads.push(scope.spawn(move || operation(index, &transport)));
        }
        threads
            .into_iter()
            .map(|thread| thread.join().expect("party thread must not panic"))
            .collect()
    });
    let stats = network.stats(started.elapsed());
    (results, stats)
}

#[test]
fn real_secp256k1_2_of_3_dkg_sign_verify_refresh_and_restore() {
    let policy = ThresholdPolicy::new(2, ["alice", "bob", "carol"].map(str::to_owned).to_vec())
        .expect("valid 2-of-3 policy");

    let (dkg_results, dkg_stats) = run_parties(3, |index, transport| {
        dkg(index, &policy, &["alice", "bob"], transport)
    });
    dkg_stats.log("DKG");
    let (shares, sessions): (Vec<EcdsaKeyShare>, Vec<SessionId>) =
        dkg_results.into_iter().map(Result::unwrap).unzip();

    assert!(sessions.windows(2).all(|pair| pair[0] == pair[1]));
    let public_key = shares[0].public_key_compressed().unwrap();
    assert_eq!(public_key.len(), 33);
    for share in &shares[1..] {
        assert_eq!(share.public_key_compressed().unwrap(), public_key);
    }

    let serialized = shares
        .iter()
        .map(EcdsaKeyShare::to_bytes)
        .collect::<Vec<_>>();
    let mut restored = serialized
        .iter()
        .map(|bytes| EcdsaKeyShare::from_bytes(bytes.expose_secret()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let detached = restored[0].detach().unwrap();
    restored[0] = EcdsaKeyShare::attach(detached).unwrap();
    assert_eq!(restored[0].public_key_compressed().unwrap(), public_key);

    let digest = [0x42_u8; 32];
    let online = vec!["alice".to_owned(), "carol".to_owned()];
    let signing_shares = [&restored[0], &restored[2]];
    let (signatures, sign_stats) = run_parties(2, |index, transport| {
        sign(
            index,
            &online,
            &policy,
            signing_shares[index],
            &digest,
            0,
            transport,
        )
    });
    sign_stats.log("SIGN");
    let signatures = signatures
        .into_iter()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    let der = signatures[0].as_deref().expect("receiver gets signature");
    assert!(signatures[1].is_none());
    let verifying_key = VerifyingKey::from_sec1_bytes(&public_key).unwrap();
    let signature = Signature::from_der(der).unwrap();
    verifying_key.verify_prehash(&digest, &signature).unwrap();

    let (refreshed, refresh_stats) = run_parties(3, |index, transport| {
        refresh(
            index,
            &policy,
            &["bob", "carol"],
            &sessions[index],
            &restored[index],
            transport,
        )
    });
    refresh_stats.log("REFRESH");
    let (new_shares, new_sessions): (Vec<EcdsaKeyShare>, Vec<SessionId>) =
        refreshed.into_iter().map(Result::unwrap).unzip();
    assert!(new_sessions.windows(2).all(|pair| pair[0] == pair[1]));
    for share in &new_shares {
        assert_eq!(share.public_key_compressed().unwrap(), public_key);
    }

    let refreshed_signing_shares = [&new_shares[1], &new_shares[2]];
    let refreshed_online = vec!["bob".to_owned(), "carol".to_owned()];
    let (refreshed_signatures, refreshed_sign_stats) = run_parties(2, |index, transport| {
        sign(
            index,
            &refreshed_online,
            &policy,
            refreshed_signing_shares[index],
            &digest,
            0,
            transport,
        )
    });
    refreshed_sign_stats.log("REFRESHED_SIGN");
    let refreshed_der = refreshed_signatures[0]
        .as_ref()
        .unwrap()
        .as_deref()
        .expect("receiver gets refreshed signature");
    let refreshed_signature = Signature::from_der(refreshed_der).unwrap();
    verifying_key
        .verify_prehash(&digest, &refreshed_signature)
        .unwrap();
}
