package main

import (
	"context"
	"fmt"
	"log"
	"sync"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/fields"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
	"k8s.io/client-go/tools/cache"
)

type ConfigReloadCallback func(data map[string]string) error

type ConfigMapWatcher struct {
	clientset kubernetes.Interface
	namespace string
	configMap string
	callback  ConfigReloadCallback
	mu        sync.RWMutex
	cancel    context.CancelFunc
}

func NewConfigMapWatcher(namespace, configMap string, callback ConfigReloadCallback) (*ConfigMapWatcher, error) {
	config, err := rest.InClusterConfig()
	if err != nil {
		return nil, fmt.Errorf("failed to get in-cluster config: %w", err)
	}

	clientset, err := kubernetes.NewForConfig(config)
	if err != nil {
		return nil, fmt.Errorf("failed to create kubernetes client: %w", err)
	}

	return &ConfigMapWatcher{
		clientset: clientset,
		namespace: namespace,
		configMap: configMap,
		callback:  callback,
	}, nil
}

func (w *ConfigMapWatcher) Start(ctx context.Context) error {
	ctx, w.cancel = context.WithCancel(ctx)

	fieldSelector := fields.OneTermEqualSelector("metadata.name", w.configMap)

	lw := cache.NewListWatchFromClient(
		w.clientset.CoreV1().RESTClient(),
		"configmaps",
		w.namespace,
		fieldSelector,
	)

	_, controller := cache.NewInformer(
		lw,
		&corev1.ConfigMap{},
		0,
		cache.ResourceEventHandlerFuncs{
			AddFunc: func(obj interface{}) {
				if cm, ok := obj.(*corev1.ConfigMap); ok {
					w.triggerReload(cm.Data)
				}
			},
			UpdateFunc: func(oldObj, newObj interface{}) {
				if cm, ok := newObj.(*corev1.ConfigMap); ok {
					w.triggerReload(cm.Data)
				}
			},
		},
	)

	go controller.Run(ctx.Done())
	log.Printf("ConfigMap watcher started for %s/%s", w.namespace, w.configMap)
	return nil
}

func (w *ConfigMapWatcher) triggerReload(data map[string]string) {
	w.mu.Lock()
	defer w.mu.Unlock()

	if err := w.callback(data); err != nil {
		log.Printf("config reload callback failed: %v", err)
		return
	}
	log.Printf("config reloaded successfully from %s/%s", w.namespace, w.configMap)
}

func (w *ConfigMapWatcher) Stop() {
	if w.cancel != nil {
		w.cancel()
	}
}

func (w *ConfigMapWatcher) GetCurrentData() (map[string]string, error) {
	cm, err := w.clientset.CoreV1().ConfigMaps(w.namespace).Get(
		context.Background(),
		w.configMap,
		metav1.GetOptions{},
	)
	if err != nil {
		return nil, fmt.Errorf("failed to get configmap: %w", err)
	}
	return cm.Data, nil
}
