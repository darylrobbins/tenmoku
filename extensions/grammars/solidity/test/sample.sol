// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "./IERC20.sol";

/// @title A small vault
/* Deposits are tracked per account. */
interface IVault {
    function deposit(uint256 amount) external;
}

library Math {
    function min(uint256 a, uint256 b) internal pure returns (uint256) {
        return a < b ? a : b;
    }
}

abstract contract Owned {
    address public owner;

    event OwnerChanged(address indexed previous, address indexed next);

    error NotOwner(address caller);

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner(msg.sender);
        _;
    }

    constructor() {
        owner = msg.sender;
    }
}

contract Vault is Owned, IVault {
    using Math for uint256;

    enum State { Open, Closed }

    struct Account {
        uint256 balance;
        bool frozen;
    }

    uint256 public constant FEE = 25;
    uint256 private constant MASK = 0xff00ff;
    string public name = "Vault \"one\"\n";
    string private note = unicode"Hello â";
    bool public live = true;
    bytes4 private selector = hex"deadbeef";
    State public state = State.Open;
    IERC20 public immutable token;
    mapping(address => Account) private accounts;

    constructor(IERC20 token_) {
        token = token_;
        accounts[msg.sender] = Account({balance: 0, frozen: false});
    }

    receive() external payable {}

    function deposit(uint256 amount) external override {
        Account storage acct = accounts[msg.sender];
        require(!acct.frozen, "frozen");
        for (uint256 i = 0; i < 3; i++) {
            acct.balance += amount.min(1e18);
        }
        while (amount > 0) {
            amount -= 1;
            if (amount == 10) break;
        }
        token.transferFrom(msg.sender, address(this), amount);
    }

    function close() external onlyOwner {
        try token.balanceOf(address(this)) returns (uint256 left) {
            state = left > 0 ? State.Open : State.Closed;
        } catch {
            state = State.Closed;
        }
        emit OwnerChanged(owner, address(0));
    }

    function size(address a) public view returns (uint256 s) {
        assembly {
            s := extcodesize(a)
        }
    }
}
