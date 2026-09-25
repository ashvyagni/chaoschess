import numpy as np
import pickle
import os

INPUT_SIZE = 777
HIDDEN1 = 256
HIDDEN2 = 128
HIDDEN3 = 64
POLICY_SIZE = 4096
LR = 0.001


def relu(x):
    return np.maximum(0, x)


def relu_grad(x):
    return (x > 0).astype(np.float32)


def softmax(x):
    e = np.exp(x - np.max(x, axis=-1, keepdims=True))
    return e / np.sum(e, axis=-1, keepdims=True)


def init_weights(fan_in, fan_out):
    limit = np.sqrt(6.0 / (fan_in + fan_out))
    return np.random.uniform(-limit, limit, (fan_in, fan_out)).astype(np.float32)


def init_bias(size):
    return np.zeros(size, dtype=np.float32)


class ChessNet:
    def __init__(self):
        self.w1 = init_weights(INPUT_SIZE, HIDDEN1)
        self.b1 = init_bias(HIDDEN1)
        self.w2 = init_weights(HIDDEN1, HIDDEN2)
        self.b2 = init_bias(HIDDEN2)
        self.w3 = init_weights(HIDDEN2, HIDDEN3)
        self.b3 = init_bias(HIDDEN3)

        self.wp = init_weights(HIDDEN3, POLICY_SIZE)
        self.bp = init_bias(POLICY_SIZE)
        self.wv = init_weights(HIDDEN3, 1)
        self.bv = init_bias(1)

        self.momentum_w = {}
        self.momentum_b = {}
        self.vel_w = {}
        self.vel_b = {}
        self.t = 0

    def forward(self, x):
        x = np.array(x, dtype=np.float32)
        if x.ndim == 1:
            x = x.reshape(1, -1)

        self.x = x
        self.z1 = x @ self.w1 + self.b1
        self.a1 = relu(self.z1)
        self.z2 = self.a1 @ self.w2 + self.b2
        self.a2 = relu(self.z2)
        self.z3 = self.a2 @ self.w3 + self.b3
        self.a3 = relu(self.z3)

        policy_logit = self.a3 @ self.wp + self.bp
        value_raw = self.a3 @ self.wv + self.bv

        self.policy_out = softmax(policy_logit)
        self.value_out = np.tanh(value_raw)

        return self.policy_out, self.value_out

    def predict(self, x):
        policy, value = self.forward(x)
        return policy[0], value[0][0]

    def train_step(self, x_batch, policy_targets, value_targets):
        self.t += 1
        batch_size = len(x_batch)

        x = np.array(x_batch, dtype=np.float32)
        pt = np.array(policy_targets, dtype=np.float32)
        vt = np.array(value_targets, dtype=np.float32).reshape(-1, 1)

        z1 = x @ self.w1 + self.b1
        a1 = relu(z1)
        z2 = a1 @ self.w2 + self.b2
        a2 = relu(z2)
        z3 = a2 @ self.w3 + self.b3
        a3 = relu(z3)

        policy_logit = a3 @ self.wp + self.bp
        policy = softmax(policy_logit)
        value = np.tanh(a3 @ self.wv + self.bv)

        policy_err = (policy - pt) / batch_size
        value_err = (value - vt) * 2.0 / batch_size

        dv = value_err * (1 - value ** 2)
        dwv = a3.T @ dv
        dbv = np.sum(dv, axis=0)

        da3 = dv @ self.wv.T
        da3 += policy_err @ self.wp.T
        dz3 = da3 * relu_grad(z3)
        dw3 = a2.T @ dz3
        db3 = np.sum(dz3, axis=0)

        da2 = dz3 @ self.w3.T
        dz2 = da2 * relu_grad(z2)
        dw2 = a1.T @ dz2
        db2 = np.sum(dz2, axis=0)

        da1 = dz2 @ self.w2.T
        dz1 = da1 * relu_grad(z1)
        dw1 = x.T @ dz1
        db1 = np.sum(dz1, axis=0)

        dwp = a3.T @ policy_err
        dbp = np.sum(policy_err, axis=0)

        grads_w = [dw1, dw2, dw3, dwv, dwp]
        grads_b = [db1, db2, db3, dbv, dbp]
        weights_w = [self.w1, self.w2, self.w3, self.wv, self.wp]
        weights_b = [self.b1, self.b2, self.b3, self.bv, self.bp]

        beta1, beta2, eps = 0.9, 0.999, 1e-8

        for i, (w, b, gw, gb) in enumerate(zip(weights_w, weights_b, grads_w, grads_b)):
            key_w = f'w{i}'
            key_b = f'b{i}'

            if key_w not in self.momentum_w:
                self.momentum_w[key_w] = np.zeros_like(w)
                self.vel_w[key_w] = np.zeros_like(w)
                self.momentum_b[key_b] = np.zeros_like(b)
                self.vel_b[key_b] = np.zeros_like(b)

            self.momentum_w[key_w] = beta1 * self.momentum_w[key_w] + (1 - beta1) * gw
            self.vel_w[key_w] = beta2 * self.vel_w[key_w] + (1 - beta2) * gw ** 2
            m_hat = self.momentum_w[key_w] / (1 - beta1 ** self.t)
            v_hat = self.vel_w[key_w] / (1 - beta2 ** self.t)
            w -= LR * m_hat / (np.sqrt(v_hat) + eps)

            self.momentum_b[key_b] = beta1 * self.momentum_b[key_b] + (1 - beta1) * gb
            self.vel_b[key_b] = beta2 * self.vel_b[key_b] + (1 - beta2) * gb ** 2
            m_hat_b = self.momentum_b[key_b] / (1 - beta1 ** self.t)
            v_hat_b = self.vel_b[key_b] / (1 - beta2 ** self.t)
            b -= LR * m_hat_b / (np.sqrt(v_hat_b) + eps)

        policy_loss = -np.sum(pt * np.log(policy + 1e-8)) / batch_size
        value_loss = np.mean((value - vt) ** 2)
        return policy_loss + value_loss

    def save(self, path):
        data = {
            'w1': self.w1, 'b1': self.b1,
            'w2': self.w2, 'b2': self.b2,
            'w3': self.w3, 'b3': self.b3,
            'wp': self.wp, 'bp': self.bp,
            'wv': self.wv, 'bv': self.bv,
            't': self.t,
        }
        with open(path, 'wb') as f:
            pickle.dump(data, f)

    def load(self, path):
        if os.path.exists(path):
            with open(path, 'rb') as f:
                data = pickle.load(f)
            self.w1 = data['w1']
            self.b1 = data['b1']
            self.w2 = data['w2']
            self.b2 = data['b2']
            self.w3 = data['w3']
            self.b3 = data['b3']
            self.wp = data['wp']
            self.bp = data['bp']
            self.wv = data['wv']
            self.bv = data['bv']
            self.t = data.get('t', 0)
            return True
        return False

    def copy_from(self, other):
        self.w1 = other.w1.copy()
        self.b1 = other.b1.copy()
        self.w2 = other.w2.copy()
        self.b2 = other.b2.copy()
        self.w3 = other.w3.copy()
        self.b3 = other.b3.copy()
        self.wp = other.wp.copy()
        self.bp = other.bp.copy()
        self.wv = other.wv.copy()
        self.bv = other.bv.copy()
        self.t = other.t
