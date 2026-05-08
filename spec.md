# Introduction

Hello. Today in this MD we will discuss the planning for writing a plan for creating Wraith!
Wraith is my pentesting tunnel tool.

I want it to be synchornous that it's main job would be to relay packets from one connection to another
Wraith will also be focused  on connecting wraiths to one another. 

At first i need a way to command a single wraith. I want a python package that will be used to command wraith. That means i want that the most basic functionality will be a python package that will allow me to listen for a wraith connection and send commands through it to this wraith.

In terms of connecting between wraiths: means i can have Wraith at 10.10.10.10 that will talk to wraith at 11.11.11.11.
Then i can relay that will work by listening on TCP on port 12345 on Wraith A and all the packets there will be passed to a connect socket on wraith B to 11.11.11.12:54321
This needs to be flexible enough to be able to do this concept but on the same wraith.
The concept is called a relay (Its basically like socat)

I also want that when Wraith A and Wraith B is connected then i can create a TAP device on wraith B.

# Terminology

## Relay
The ability like in socat to relay data from one socket to another one 
## TAP
The ability to sniff from any wraith that is eventually connected back home

## Home
The home is the place where our C2 runs and commands are being sent from.
## PyWraith
The python package used to serialize protobufs to send to wraith. Essentially can be used to communicate with wraith
The main functionality would be to serialize and handle sending commands to wraith
Do not embed to the core of it handling it's own C2 server. It can be more as utility

# Tech stack
* Rust
* Protobuf
* Python homeside integration
* Nexus and outpost integration
* Yamux (If able)

# Requirements
## First stage
* PyWraith setup that can command wraith and pass commands to it
* A wraith that can establish connection and get commands and run them
* An ability to relay data

## Second stage
* Connecting wraiths with one another (Here and only here yamux should be invlovede)
* TAP device creation

# Binary Arguments
To run wraith i want these options:
* ./wraith command listen {IP}:{PORT} - Will be used to bind and listen for the C2 to connect to wraith
* ./wraith command connect {IP}:{PORT} - Will be used to connect to the C2

* ./wraith agent listen {IP}:{PORT} - Will be used to bind and listen for another wraith to connect (stage 2)
* ./wraith agent connect {IP}:{PORT} - Will be used to connect to another wraith that is already listening (Stage 2)

**NOTE: These arguments are just startup arguments. One of them is required in order for wraith to be able to get commands.**

# Commands
* create_relay - Gets two socket parameters and create a relay (Like socat). Returns ID
* delete_relay - Delete a relay by it's id
* list_relays - List the relays you have
* wraith_listen - Listen for a wraith to connect to you (Stage 2)
* wraith_connect - Connect to another wraith that is listening (Stage 2)

# Modules
## Rust side
Create an src/ folder with all the files there

From there we need

* main.rs - Will be in charge of the entrypoint. par
* relay module - This will be in charge of 
* commands/command_handler module - Handles all the receiving of commands (Protobuf) and calling the relevant modules (Relay/Tunnel)
* tunnel - In charge of connecting wraiths to one another and form a wraith tunnel (Stage 2)

# References
/home/user/tools/shelly - This is a reverse shell written for linux that you can take a bit of inspiration in terms of the protobuf usage and pyshelly