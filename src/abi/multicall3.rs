use alloy::sol;

sol! {
    struct Call3 {
        // Target contract to call.
        address target;
        // If false, the entire call will revert if the call fails.
        bool allowFailure;
        // Data to call on the target contract.
        bytes callData;
    }

    struct Result {
        // True if the call succeeded, false otherwise.
        bool success;
        // Return data if the call succeeded, or revert data if the call reverted.
        bytes returnData;
    }

    /// @notice Aggregate calls, ensuring each returns success if required
    /// @param calls An array of Call3 structs
    /// @return returnData An array of Result structs
    function aggregate3(Call3[] calldata calls) public payable returns (Result[] memory returnData);

    function getCurrentBlockTimestamp() public view returns (uint256 timestamp);
}
