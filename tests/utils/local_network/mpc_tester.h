#include <gtest/gtest.h>

#include "mpc_runner.h"

namespace coinbase::testutils {

class Network2PC : public testing::Test {
 protected:
  void SetUp() override {
    auto job1 = std::make_shared<mpc::job_session_2p_t>(mpc::party_t::p1, mpc_runner_t::test_pids[0],
                                                        mpc_runner_t::test_pids[1], nullptr, 0);
    auto job2 = std::make_shared<mpc::job_session_2p_t>(mpc::party_t::p2, mpc_runner_t::test_pids[0],
                                                        mpc_runner_t::test_pids[1], nullptr, 0);
    mpc_runner = std::make_unique<mpc_runner_t>(job1, job2);
  }

  std::unique_ptr<mpc_runner_t> mpc_runner;
};

class Network4PC : public testing::Test {
 protected:
  void SetUp() override { mpc_runner = std::make_unique<mpc_runner_t>(4); }

  std::unique_ptr<mpc_runner_t> mpc_runner;
};

class NetworkMPC : public testing::TestWithParam<int> {
 protected:
  void SetUp() override {
    int n_parties = GetParam();
    std::vector<std::shared_ptr<mpc::job_session_mp_t>> jobs(n_parties);
    std::vector<crypto::bn_t> pids(mpc_runner_t::test_pids.begin(), mpc_runner_t::test_pids.begin() + n_parties);
    for (int i = 0; i < n_parties; i++) {
      jobs[i] = std::make_shared<mpc::job_session_mp_t>(mpc::party_idx_t(i), pids, nullptr, 0);
    }
    mpc_runner = std::make_unique<mpc_runner_t>(jobs);
  }

  std::unique_ptr<mpc_runner_t> mpc_runner;
};

}  // namespace coinbase::testutils