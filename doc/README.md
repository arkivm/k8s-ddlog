# k8s monitoring

# Setup

## Components

- Install Docker [https://www.docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop)
- Install minikube (single-node cluster) [https://minikube.sigs.k8s.io/docs/start/](https://minikube.sigs.k8s.io/docs/start/)
- Install Kind (multi-node cluster) [https://kind.sigs.k8s.io/docs/user/quick-start/#installation](https://kind.sigs.k8s.io/docs/user/quick-start/#installation)

## Creating docker image

The running example creates a webapp that sends a response to a HTTP request (borrowed from "Kubernetes in action" book)

- Create a docker image that listens on a HTTP socket
- `DockerFile`

    ```docker
    FROM node:7
    ADD app.js /app.js
    ENTRYPOINT ["node", "app.js"]
    ```

- `app.js`

    ```jsx
    const http = require('http');
    const os = require('os');

    console.log('Kubia server starting ...');

    var handler = function(req, resp) {
      console.log("Received request from " + req.connection.remoteAddress);
      resp.writeHead(200);
      resp.end("You've hit the server " + os.hostname + "\n");
    };

    var www = http.createServer(handler);
    www.listen(8080);
    ```

- To install docker image locally

     `eval $(minikube -p minikube docker-env)`

- Build the image

    `docker build -t kubia .`

- This command should install the docker image locally

## Creating a replication controller

- Create a replication controller configuration with the above image that spawns 3 replicas (`kubia-rc.yaml`)

    ```yaml
    apiVersion: v1
    kind: ReplicationController
    metadata:
      name: kubia
    spec:
      replicas: 3
      selector:
        app: kubia
      template:
        metadata:
          labels:
            app: kubia
        spec:
          containers:
          - name: kubia
            image: kubia
            imagePullPolicy: Never
            ports:
            - containerPort: 8080
    ```

- Create the replication controller

    ```bash
    kubectl create -f ./kubia-rc.yaml
    ```

- If pods are successfully created, you should see

    ```bash
    $ kubectl get pod # you may see different pod names
    kubia-f22js   1/1     Running   0          136m
    kubia-n67gj   1/1     Running   0          136m
    kubia-r5xvd   1/1     Running   0          136m

    $ kubectl get rc # gets the list of replication controllers
    NAME    DESIRED   CURRENT   READY   AGE
    kubia   3         3         3       138m

    $ kubectl get node # get the node (in our case, it's minikube)
    NAME       STATUS   ROLES                  AGE     VERSION
    minikube   Ready    control-plane,master   3d17h   v1.21.2
    ```

## Creating a multi-node cluster
[1] [https://joe.blog.freemansoft.com/2020/07/multi-node-kubernetes-with-kind-and.html](https://joe.blog.freemansoft.com/2020/07/multi-node-kubernetes-with-kind-and.html)
- `minikube` apparently supports only a single-node cluster. However, with `kind`, one can create a multi-node cluster.
- Create a configuration file stating how many controller nodes and worker nodes you want on the cluster

    ```yaml
    kind: Cluster
    apiVersion: kind.x-k8s.io/v1alpha4
    nodes:
      - role: control-plane
      - role: worker
      - role: worker
    ```

    There are some hiccups when you want to access the same cluster after restarting your docker or the machine. There are open issues that somethings are not supported after reboot ([https://github.com/kubernetes-sigs/kind/issues/1689](https://github.com/kubernetes-sigs/kind/issues/1689))

- Create the same set of pods we created using `minikube`. Since we used the docker env that points to our local docker registry for `minikube`, it worked without passing any credentials to the pod spec. However, with the switch to `kind`, we have to create a secret for pulling images from dockerhub.
    - Create secret with docker credentials

        ```bash
        kubectl create secret docker-registry my-secret \
        		--docker-server=https://index.docker.io/v1/ \
        		--docker-username=username \
        		--docker-password=blahblah \
            --docker-email=none@none.com
        ```

    - Use the created secret for pulling images inside the pod

        ```yaml
        apiVersion: v1
        kind: ReplicationController
        metadata:
          name: kubia
        spec:
          replicas: 3
          selector:
            app: kubia
          template:
            metadata:
              labels:
                app: kubia
            spec:
              containers:
              - name: kubia
                image: luksa/kubia
                ports:
                - containerPort: 8080
              imagePullSecrets:
                - name: my-secret
        ```

    ### Assigning pods to nodes

    [2] [https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/](https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/)

    1. `nodeSelector`
        - Assign a label to a node

            ```bash
            # kubectl label nodes nodename label_key=label_value
            $ kubectl label nodes node-worker disktype=ssd
            ```

        - Use the same label as `nodeSelector` in pod spec

            ```yaml
            apiVersion: v1
            kind: Pod
            metadata:
              name: nginx
              labels:
                env: test
            spec:
              containers:
              - name: nginx
                image: nginx
                imagePullPolicy: IfNotPresent
              imagePullSecrets:
              - name: my-secret
              nodeSelector:
                disktype: ssd
            ```

            - Then the pod would be scheduled on the node that has this label `disktype=ssd`

    2. Using node Affinity
      Affinity provides much more expressivity when it comes to specifying constraints. More details (here)[https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/]

      Here is an example Pod that specifies a `NodeAffinity` in the configuration
      ```yaml
      apiVersion: v1
      kind: Pod
      metadata:
        name: nginx-withaffinity
        labels:
          env: test
          security: S1
      spec:
        containers:
        - name: nginx
          image: nginx
          imagePullPolicy: IfNotPresent
        imagePullSecrets:
        - name: my-secret
        affinity:
          nodeAffinity:
            requiredDuringSchedulingIgnoredDuringExecution:
              nodeSelectorTerms:
              - matchExpressions:
                - key: kubernetes.io/e2e-az-name
                  operator: In
                  values:
                  - e2e-az1
                  - e2e-az2
      ```

    3. using `PodAffinity` and `PodAntiAffinity`
    Inter-pod affinity and anti-affinity allow you to specify constraints such that the pod would be scheduled on a node that has at least one pod
    that satifies these labels.

    Here, we create a Pod that affines to the above created pod `nginx-withaffinity` (check labels `security: S1` in the above yaml file)
    ```yaml
    apiVersion: v1
    kind: Pod
    metadata:
      name: nginx-with-pod-affinity
    spec:
      containers:
      - name: nginx
        image: nginx
        imagePullPolicy: IfNotPresent
      imagePullSecrets:
      - name: my-secret
      affinity:
        podAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
          - labelSelector:
              matchExpressions:
              - key: security
                operator: In
                values:
                - S1
            topologyKey: topology.kubernetes.io/zone
        ```
