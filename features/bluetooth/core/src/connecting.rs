use super::*;

impl Shared {
    pub(super) fn device(&self, address: &str) -> Result<(OwnedObjectPath, bool, bool), String> {
        if !bluez::is_address(address) {
            return Err(format!("{address:?} isn't a Bluetooth address"));
        }
        let state = lock(&self.state);
        let device = state.objects.device(address).ok_or("Out of range")?;
        Ok((device.path, device.paired, device.connected))
    }

    pub(super) fn connect_classic(
        &self,
        address: &str,
        path: &OwnedObjectPath,
        error: zbus::Error,
    ) -> zbus::Result<()> {
        let profile =
            lock(&self.state).objects.device(address).and_then(|device| device.classic_profile().map(str::to_owned));
        let Some(profile) = profile else { return Err(error) };
        self.logger.info(LOG_SOURCE, &format!("{address}: LE connect aborted, connecting over classic Bluetooth"));
        self.connect_profile(path, &profile)
    }

    pub(super) fn connect_profile(&self, path: &OwnedObjectPath, profile: &str) -> zbus::Result<()> {
        let agent = lock(&self.state).agent.clone().ok_or_else(not_followed)?;
        zbus::block_on(agent.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "ConnectProfile", &(profile,)))
            .map(|_| ())
    }

    pub(super) fn ensure_a2dp(&self, address: &str, path: &OwnedObjectPath) {
        {
            let mut state = lock(&self.state);
            if !state.objects.device(address).is_some_and(|device| device.offers_a2dp()) {
                return;
            }
            state.connects.remove(&address.to_uppercase());
        }
        for attempt in 0..=AUDIO_RETRIES {
            if attempt > 0 {
                if self.go_on(address).is_err() || lock(&self.state).stopped {
                    return;
                }
                self.logger.info(LOG_SOURCE, &format!("{address}: connected without A2DP, asking for it again"));
                if let Err(e) = self.connect_profile(path, bluez::A2DP_SINK) {
                    self.logger.info(LOG_SOURCE, &format!("{address}: A2DP refused: {e}"));
                }
            }
            if self.wait_for_a2dp(path) {
                return;
            }
        }
        self.logger.info(LOG_SOURCE, &format!("{address}: connected, but A2DP never came up"));
    }

    pub(super) fn wait_for_a2dp(&self, path: &OwnedObjectPath) -> bool {
        let deadline = Instant::now() + self.sources.audio_wait;
        let mut state = lock(&self.state);
        loop {
            if state.objects.a2dp(path.as_str()) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || state.stopped {
                return false;
            }
            state = self.wake.wait_timeout(state, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }

    pub(super) fn cancel(&self, path: &OwnedObjectPath, status: Option<BluetoothStatus>) {
        let method = match status {
            Some(BluetoothStatus::Pairing) => "CancelPairing",
            _ => "Disconnect",
        };
        let result = self.calls().and_then(|calls| zbus::block_on(call(&calls, path, DEVICE, method)));
        if let Err(e) = result {
            self.logger.info(LOG_SOURCE, &format!("can't stop what {path} was doing: {e}"));
        }
    }

    pub(super) fn limit_connect(shared: &Arc<Self>, address: &str, path: &OwnedObjectPath) {
        let key = address.to_uppercase();
        let number = {
            let mut state = lock(&shared.state);
            state.last_connect += 1;
            let number = state.last_connect;
            state.connects.insert(key.clone(), number);
            number
        };
        let (shared, path) = (Arc::clone(shared), path.clone());
        std::thread::spawn(move || {
            let running = |state: &State| {
                state.connects.get(&key) == Some(&number) && state.busy.get(&key) == Some(&BluetoothStatus::Connecting)
            };
            let (mut state, _) = shared
                .wake
                .wait_timeout_while(lock(&shared.state), shared.sources.connect_limit, |state| running(state))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(calls) = state.calls.clone().filter(|_| running(&state)) else { return };
            state.timed_out.insert(key.clone());
            shared.logger.info(LOG_SOURCE, &format!("{key}: connect took too long, stopping it"));
            if let Err(e) = zbus::block_on(call(&calls, &path, DEVICE, "Disconnect")) {
                shared.logger.info(LOG_SOURCE, &format!("can't stop connecting {path}: {e}"));
            }
            drop(state);
        });
    }

    pub(super) fn go_on(&self, address: &str) -> zbus::Result<()> {
        match lock(&self.state).halted.contains(&address.to_uppercase()) {
            true => Err(zbus::Error::Failure("stopped by the user".into())),
            false => Ok(()),
        }
    }

    pub(super) fn set_busy(&self, address: &str, status: BluetoothStatus) {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.errors.remove(&key);
        state.busy.insert(key, status);
        self.publish(&mut state);
    }

    pub(super) fn done(&self, address: &str, result: zbus::Result<()>) -> Result<(), String> {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.busy.remove(&key);
        state.connects.remove(&key);
        let result = if state.timed_out.remove(&key) {
            Err(zbus::Error::FDO(Box::new(zbus::fdo::Error::NoReply("connect took too long".into()))))
        } else if state.halted.remove(&key) {
            Ok(())
        } else {
            result
        };
        let shown = state.pairing.as_ref().is_some_and(|asked| asked.slot.is_none() && asked.request.address == key);
        if shown {
            state.pairing = None;
        }
        let outcome = match result {
            Ok(()) => Ok(()),
            Err(e) => {
                self.logger.info(LOG_SOURCE, &format!("{address}: {e}"));
                match bluez::failure(&e) {
                    Some(words) => {
                        state.errors.insert(key, words.clone());
                        Err(words)
                    }
                    None => Ok(()),
                }
            }
        };
        self.publish(&mut state);
        outcome
    }
}
